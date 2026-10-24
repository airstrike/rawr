//! Symbolic imported contract integration for Rawr package compilation.

use rawr_import::classify::representability::{self, Representability};

use crate::{diagnostic, host, receiver, ImmutableString};

#[doc(hidden)]
pub mod runner;

/// Same-pass compilation against metadata owned by the retained native compiler.
#[cfg(not(feature = "no_function"))]
#[doc(hidden)]
pub mod resident;

/// Unforgeable Rawr ownership brand retained by every Engine-sealed demand.
///
/// The private field prevents external callers from minting Rawr-branded
/// importer evidence:
///
/// ```compile_fail
/// let _brand = rawr::package::import::Brand { _private: () };
/// ```
///
/// ```compile_fail
/// # fn clone_brand(brand: &rawr::package::import::Brand) {
/// let _: rawr::package::import::Brand = brand.clone();
/// # }
/// ```
///
/// ```compile_fail
/// let _: rawr::package::import::Brand = Default::default();
/// ```
pub struct Brand {
    facade: Box<[u8]>,
}

/// Atomic compiler-only imported demand plus normalized application contract.
#[doc(hidden)]
pub struct ApplicationDemand {
    demand: rawr_import::Demand<rawr_import::Sealed, Brand>,
    application: crate::application::CheckedContract,
    named: Box<[Box<str>]>,
}

struct CompiledImported {
    demand: rawr_import::Demand<rawr_import::Sealed, Brand>,
    application: Option<crate::application::CheckedContract>,
    named: Box<[Box<str>]>,
}

impl ApplicationDemand {
    /// Whether the Rawr source still names Rust `path`, an item below it,
    /// or (for a demanded member) its owner.
    #[doc(hidden)]
    #[must_use]
    pub fn names(&self, path: &str) -> bool {
        let below = |long: &str, short: &str| {
            long.strip_prefix(short)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with("::"))
        };
        self.named
            .iter()
            .any(|named| below(named, path) || below(path, named))
    }

    /// Consume the exact staging products together.
    #[doc(hidden)]
    pub fn into_parts(
        self,
    ) -> (
        rawr_import::Demand<rawr_import::Sealed, Brand>,
        crate::application::CheckedContract,
    ) {
        (self.demand, self.application)
    }
}

impl Brand {
    fn mint(facade: Box<[u8]>) -> Self {
        Self { facade }
    }

    /// Reconstruct the Rawr ownership brand from a validated cached facade node.
    #[doc(hidden)]
    pub fn replay(facade: &rawr_import::surface::facade::Cached) -> Result<Self, Error> {
        let facade = facade.bytes();
        let mut retained = Vec::new();
        retained
            .try_reserve_exact(facade.len())
            .map_err(|_| Error::Allocation)?;
        retained.extend_from_slice(facade);
        Ok(Self::mint(retained.into_boxed_slice()))
    }
}

impl rawr_import::surface::Runner for Brand {
    fn compile_snapshot(&self) -> bool {
        true
    }

    fn facade_plan(&self) -> Option<&[u8]> {
        Some(&self.facade)
    }
}

/// Failure to transform a classified importer contract into compiler facades.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The canonical Rawr target can no longer be discovered.
    #[error(transparent)]
    Project(#[from] crate::project::Error),
    /// Exact Rawr snapshot evidence changed during symbolic compilation.
    #[error(transparent)]
    Snapshot(#[from] crate::reload::source::Error),
    /// The imported demand could not be bounded or sealed.
    #[error(transparent)]
    Surface(#[from] rawr_import::surface::Error),
    /// Semantic selector derivation could not preserve the classified contract.
    #[error(transparent)]
    Classification(#[from] rawr_import::classify::Error),
    /// Rediscovery produced bytes other than the caller's exact snapshot.
    #[error("Rawr package inputs no longer match the supplied snapshot")]
    SnapshotMismatch,
    /// A compiler use site could not be projected into exact source bytes.
    #[error("cannot position imported use `{path}` in `{source_identity}`")]
    SourceEvidence {
        /// Fully qualified imported path.
        path: Box<str>,
        /// Canonical Rawr source identity.
        source_identity: Box<str>,
    },
    /// Checked lowering produced an imported operation absent from the exact contract.
    #[error("unmapped imported use `{path}` with selector {selector:?}")]
    UnmappedImportedUse {
        /// Fully qualified imported path.
        path: Box<str>,
        /// Closed semantic operation selector.
        selector: rawr_import::surface::identity::Selector,
    },
    /// The contract ledgers no longer have exact one-to-one cardinality.
    #[error("imported contract ledger cardinality changed")]
    Cardinality,
    /// Imported contract lookup evidence could not reserve its exact capacity.
    #[error("cannot allocate imported contract lookup evidence")]
    Allocation,
    /// The classified contract contains a path shape the facade cannot name.
    #[error("invalid imported facade path `{path}`")]
    InvalidPath {
        /// Rejected unchanged Rust path.
        path: Box<str>,
    },
    /// Static facade validation rejected the imported proof.
    #[error(transparent)]
    Host(#[from] host::Error),
}

impl crate::Engine {
    /// Compile one classified Rust API contract against exact Rawr source bytes.
    ///
    /// This transform performs static name resolution and body type-checking
    /// only. It neither links nor invokes imported Rust code.
    pub fn compile_imported(
        &self,
        contract: &rawr_import::Contract<rawr_import::Classified>,
        snapshot: crate::reload::source::Snapshot,
    ) -> Result<rawr_import::Demand<rawr_import::Sealed, Brand>, super::Error> {
        let limits = rawr_import::cargo::Limits::standard();
        self.compile_imported_with_limits(contract, snapshot, &limits, false, None, None)
            .map(|compiled| compiled.demand)
    }

    /// Compile one imported contract against profile facades already installed in this engine.
    ///
    /// The caller must have reconstructed those facades from the exact encoded contract. This
    /// entry exists for linked framework profiles that replace only semantic anchors while the
    /// importer remains authoritative for ordinary declarations and selected operations.
    #[doc(hidden)]
    pub fn compile_imported_with_installed_profile(
        &self,
        contract: &rawr_import::Contract<rawr_import::Classified>,
        snapshot: crate::reload::source::Snapshot,
    ) -> Result<rawr_import::Demand<rawr_import::Sealed, Brand>, super::Error> {
        let limits = rawr_import::cargo::Limits::standard();
        self.compile_imported_with_limits(contract, snapshot, &limits, true, None, None)
            .map(|compiled| compiled.demand)
    }

    /// Atomically compile symbolic imported demand and one normalized
    /// application contract under the same exact snapshot and staging facade.
    #[doc(hidden)]
    pub fn compile_imported_application_with_installed_profile(
        &self,
        contract: &rawr_import::Contract<rawr_import::Classified>,
        snapshot: crate::reload::source::Snapshot,
        cycle: &host::facade::ProfileCycle,
        event_projection: rawr_import::profile::contract::EventProjectionRules,
    ) -> Result<ApplicationDemand, super::Error> {
        let limits = rawr_import::cargo::Limits::standard();
        let compiled = self.compile_imported_with_limits(
            contract,
            snapshot,
            &limits,
            true,
            Some(cycle),
            Some(event_projection),
        )?;
        Ok(ApplicationDemand {
            demand: compiled.demand,
            application: compiled
                .application
                .expect("requested application staging retains its checked contract"),
            named: compiled.named,
        })
    }

    fn compile_imported_with_limits(
        &self,
        contract: &rawr_import::Contract<rawr_import::Classified>,
        snapshot: crate::reload::source::Snapshot,
        limits: &rawr_import::cargo::Limits,
        installed_profile: bool,
        cycle: Option<&host::facade::ProfileCycle>,
        event_projection: Option<rawr_import::profile::contract::EventProjectionRules>,
    ) -> Result<CompiledImported, super::Error> {
        snapshot
            .revalidate()
            .map_err(Error::from)
            .map_err(super::Error::import)?;
        let imported_snapshot = snapshot
            .imported(limits)
            .map_err(Error::from)
            .map_err(super::Error::import)?;

        let target = crate::project::Target::discover(snapshot.entry())
            .map_err(Error::from)
            .map_err(super::Error::import)?;
        let graph = super::Graph::load(self, &target)?;
        let current_source = crate::reload::source::Graph::from_package(&graph);
        let current_snapshot = crate::reload::source::Snapshot::from_graph(&current_source);
        if !snapshot.exactly_matches(&current_snapshot) {
            return Err(super::Error::import(Error::SnapshotMismatch));
        }

        let sources = graph.diagnostic_sources();
        let mut allocation = contract
            .allocation(limits)
            .map_err(Error::from)
            .map_err(super::Error::import)?;
        let facade = rawr_import::surface::facade::Plan::encode(contract, limits)
            .map_err(Error::from)
            .map_err(super::Error::import)?;
        allocation
            .retain(1, facade.len())
            .map_err(Error::from)
            .map_err(super::Error::import)?;
        let paths = imported_paths(contract, &mut allocation).map_err(super::Error::import)?;
        let mut registry = self
            .host_registry
            .clone_for_import(&mut allocation)
            .map_err(Error::from)
            .map_err(super::Error::import)?;
        if !installed_profile {
            install(contract, &mut registry, &mut allocation).map_err(super::Error::import)?;
        }
        let program = match crate::resolve::package::resolve(graph, &registry) {
            Ok(program) => program,
            Err(failure) => {
                if let Some(report) = imported_resolution_report(&failure, &sources, &paths)
                    .map_err(super::Error::diagnostic)?
                {
                    return Err(super::Error::import_diagnostic(report));
                }
                let demand = match rust_miss(contract, &failure.error) {
                    Some(Miss::Unresolved(message)) => {
                        let source = sources
                            .get(failure.primary.index())
                            .ok_or(diagnostic::Error::EmptySourceIdentity)
                            .map_err(super::Error::diagnostic)?;
                        let report = diagnostic::Report::error(
                            diagnostic::code::RESOLVE_UNKNOWN_IMPORT,
                            message,
                        )
                        .primary(source.clone(), failure.error.primary_span(), None)
                        .map_err(super::Error::diagnostic)?
                        .finish();
                        return Err(super::Error::import_diagnostic(report));
                    }
                    Some(Miss::Demand(path, through_alias)) => Some((path, through_alias)),
                    None => None,
                };
                let error = super::Error::resolution(sources.clone(), failure);
                return Err(match demand {
                    Some((path, through_alias)) => error.with_rust_demand(&path, through_alias),
                    None => error,
                });
            }
        };
        let symbolic =
            match crate::resolve::lower_imported(self, &program, &registry, &mut allocation) {
                Ok(uses) => uses,
                Err(crate::resolve::ImportedFailure::Surface(error)) => {
                    return Err(super::Error::import(Error::Surface(error)));
                }
                Err(crate::resolve::ImportedFailure::Lowering(failure)) => {
                    let (evidence, error) = failure.into_parts();
                    let demand = match error.err_type() {
                        crate::ParseErrorType::Resolution(resolution) => {
                            match rust_miss(contract, resolution) {
                                Some(Miss::Demand(path, through_alias)) => {
                                    Some((path, through_alias))
                                }
                                _ => None,
                            }
                        }
                        _ => None,
                    };
                    if let Some(report) =
                        imported_lowering_report(&evidence, &error, &sources, &paths)
                            .map_err(super::Error::diagnostic)?
                    {
                        let error = super::Error::import_diagnostic(report);
                        return Err(match demand {
                            Some((path, through_alias)) => {
                                error.with_rust_demand(&path, through_alias)
                            }
                            None => error,
                        });
                    }
                    let error = super::Error::compilation(sources.clone(), evidence, error);
                    return Err(match demand {
                        Some((path, through_alias)) => error.with_rust_demand(&path, through_alias),
                        None => error,
                    });
                }
            };
        let application = match cycle {
            Some(cycle) => {
                let evidence = crate::program::ContractEvidence::from_resolved(
                    &program,
                    symbolic.dependencies,
                );
                match crate::application::CheckedContract::transform(&evidence, cycle) {
                    Ok(application) => Some(application),
                    Err(error) => {
                        let source = evidence.application_error_source(&error);
                        return Err(super::Error::resolution(
                            sources.clone(),
                            crate::resolve::package::Failure {
                                primary: crate::package::source::Id::from_index(source.index()),
                                secondary: None,
                                error: crate::resolve::Error::from(error),
                            },
                        ));
                    }
                }
            }
            None => None,
        };

        let compiler = contract
            .compiler_with_allocation(Brand::mint(facade), imported_snapshot, limits, allocation)
            .map_err(Error::from)
            .map_err(super::Error::import)?;
        let mut compiler = match event_projection {
            Some(rules) => compiler
                .with_event_projection_rules(rules)
                .map_err(Error::from)
                .map_err(super::Error::import)?,
            None => compiler,
        };
        let mut named = Vec::new();
        for used in symbolic.uses {
            named.push(Box::<str>::from(used.path.as_str()));
            let source = program
                .source()
                .table()
                .resolve(program.source().key(used.source))
                .map_err(|_| Error::SourceEvidence {
                    path: used.path.as_str().into(),
                    source_identity: "<foreign source table>".into(),
                })
                .map_err(super::Error::import)?;
            if let Some(site) = used.endpoint {
                let exact = installed_profile
                    && registry
                        .operation_endpoint(used.path.as_str())
                        .is_some_and(|(endpoint, _)| endpoint.site() == &site);
                if !exact {
                    return Err(super::Error::import(Error::UnmappedImportedUse {
                        path: used.path.as_str().into(),
                        selector: used.selector,
                    }));
                }
                continue;
            }
            let mut identity = None;
            for outcome in paths.outcomes(used.path.as_str()) {
                if !contract.accepts(outcome.identity(), &used.selector) {
                    continue;
                }
                if identity.replace(outcome.identity()).is_some() {
                    return Err(super::Error::import(Error::Cardinality));
                }
            }
            let identity = match identity {
                Some(identity) => identity,
                None => {
                    let report = diagnostic::Report::error(
                        diagnostic::code::RESOLVE_HOST_OPERATION,
                        Error::UnmappedImportedUse {
                            path: used.path.as_str().into(),
                            selector: used.selector,
                        }
                        .to_string(),
                    )
                    .primary(
                        diagnostic::Source::new(source.identity(), source.text())
                            .map_err(super::Error::diagnostic)?,
                        used.span,
                        Some("this symbolic imported use has no exact classified selector".into()),
                    )
                    .map_err(super::Error::diagnostic)?
                    .finish();
                    let mut outcomes = Vec::new();
                    let retained = paths.outcomes(used.path.as_str());
                    outcomes
                        .try_reserve_exact(retained.len())
                        .map_err(|_| super::Error::import(Error::Allocation))?;
                    outcomes.extend(retained);
                    let report = augment_external(report.clone(), outcomes)
                        .map_err(super::Error::diagnostic)?
                        .unwrap_or(report);
                    return Err(super::Error::import_diagnostic(report));
                }
            };
            let (start, end) =
                crate::resolve::lower_nominal::imported_range(source.text(), used.span)
                    .ok_or_else(|| Error::SourceEvidence {
                        path: used.path.as_str().into(),
                        source_identity: source.identity().into(),
                    })
                    .map_err(super::Error::import)?;
            if used.types.is_empty() && used.functions.is_empty() {
                compiler
                    .require_selected(identity, used.selector, source.identity(), start, end)
                    .map_err(Error::from)
                    .map_err(super::Error::import)?;
                continue;
            }
            let mut functions = Vec::new();
            functions
                .try_reserve_exact(used.functions.len())
                .map_err(|_| super::Error::import(Error::Allocation))?;
            for function in used.functions {
                named.push(Box::<str>::from(function.path.as_str()));
                let mut selected = None;
                for outcome in paths.outcomes(function.path.as_str()) {
                    if !contract.accepts(outcome.identity(), &function.selector) {
                        continue;
                    }
                    if selected.replace(outcome.identity()).is_some() {
                        return Err(super::Error::import(Error::Cardinality));
                    }
                }
                let selected = selected.ok_or_else(|| {
                    super::Error::import(Error::UnmappedImportedUse {
                        path: function.path.as_str().into(),
                        selector: function.selector.clone(),
                    })
                })?;
                let (function_start, function_end) =
                    crate::resolve::lower_nominal::imported_range(source.text(), function.span)
                        .ok_or_else(|| Error::SourceEvidence {
                            path: function.path.as_str().into(),
                            source_identity: source.identity().into(),
                        })
                        .map_err(super::Error::import)?;
                functions.push(
                    rawr_import::surface::instance::FunctionItem::selected(
                        function.position,
                        selected,
                        function.selector,
                        function_start,
                        function_end,
                    )
                    .map_err(Error::from)
                    .map_err(super::Error::import)?,
                );
            }
            let mut mappers = Vec::new();
            mappers
                .try_reserve_exact(used.mappers.len())
                .map_err(|_| super::Error::import(Error::Allocation))?;
            for mapper in used.mappers {
                let (mapper_start, mapper_end) =
                    crate::resolve::lower_nominal::imported_range(source.text(), mapper.span)
                        .ok_or_else(|| Error::SourceEvidence {
                            path: used.path.as_str().into(),
                            source_identity: source.identity().into(),
                        })
                        .map_err(super::Error::import)?;
                mappers.push(
                    rawr_import::surface::instance::EventMapper::selected(
                        mapper.position,
                        mapper.seal,
                        mapper_start,
                        mapper_end,
                    )
                    .map_err(Error::from)
                    .map_err(super::Error::import)?,
                );
            }
            let instance = rawr_import::surface::instance::Arguments::new(
                used.types,
                Box::default(),
                Box::default(),
                None,
                limits,
            )
            .and_then(|instance| instance.with_function_items(functions.into_boxed_slice(), limits))
            .and_then(|instance| instance.with_event_mappers(mappers.into_boxed_slice(), limits))
            .map_err(Error::from)
            .map_err(super::Error::import)?;
            compiler
                .require_instance(
                    identity,
                    used.selector,
                    instance,
                    source.identity(),
                    start,
                    end,
                )
                .map_err(Error::from)
                .map_err(super::Error::import)?;
        }

        snapshot
            .revalidate()
            .map_err(Error::from)
            .map_err(super::Error::import)?;
        let demand = compiler
            .seal()
            .map_err(Error::from)
            .map_err(super::Error::import)?;
        named.sort_unstable();
        named.dedup();
        Ok(CompiledImported {
            demand,
            application,
            named: named.into_boxed_slice(),
        })
    }
}

fn imported_resolution_report(
    failure: &crate::resolve::package::Failure,
    sources: &[diagnostic::Source],
    paths: &ImportedPaths<'_>,
) -> Result<Option<diagnostic::Report>, diagnostic::Error> {
    let crate::resolve::Error::UnknownImport { path, reference } = &failure.error else {
        let Some((path, namespace)) = diagnostic_path(&failure.error) else {
            return Ok(None);
        };
        let outcomes = diagnostic_outcomes(paths, &path, namespace)?;
        if outcomes.is_empty() {
            return Ok(None);
        }
        let primary = sources
            .get(failure.primary.index())
            .ok_or(diagnostic::Error::EmptySourceIdentity)?;
        let secondary = failure
            .secondary
            .and_then(|source| sources.get(source.index()))
            .cloned();
        let report =
            diagnostic::Report::from_resolution(primary.clone(), secondary, &failure.error)?;
        return augment_external(report, outcomes);
    };
    let outcomes = paths.outcomes(path.as_str());
    if outcomes.len() == 0 {
        return Ok(None);
    }
    let mut evidence = Vec::new();
    evidence
        .try_reserve_exact(outcomes.len())
        .map_err(|_| diagnostic::Error::Allocation)?;
    for outcome in outcomes {
        if let Some(retained) = unavailable_evidence(outcome) {
            evidence.push(retained);
        }
    }
    evidence.sort_unstable_by(|left, right| {
        left.reason
            .cmp(&right.reason)
            .then_with(|| left.source.cmp(right.source))
            .then_with(|| left.declaration.cmp(right.declaration))
    });
    evidence.dedup_by(|left, right| {
        left.reason == right.reason
            && left.source == right.source
            && left.declaration == right.declaration
    });
    if evidence.is_empty() {
        return Ok(None);
    }
    let reason = joined_reasons(&evidence)?;
    let source = sources
        .get(failure.primary.index())
        .ok_or(diagnostic::Error::EmptySourceIdentity)?;
    let mut builder = diagnostic::Report::error(
        diagnostic::code::RESOLVE_UNKNOWN_IMPORT,
        format!("imported Rust path `{path}` is unavailable: {reason}"),
    )
    .primary(
        source.clone(),
        *reference,
        Some("Rawr use requires this imported Rust path".into()),
    )?;
    let mut external = Vec::new();
    let external_capacity = evidence
        .len()
        .checked_mul(2)
        .ok_or(diagnostic::Error::Allocation)?;
    external
        .try_reserve_exact(external_capacity)
        .map_err(|_| diagnostic::Error::Allocation)?;
    for retained in &evidence {
        external.push((retained.source, "Rust export is retained here"));
        if retained.declaration != retained.source {
            external.push((
                retained.declaration,
                "Rust declaration or policy evidence is retained here",
            ));
        }
    }
    external.sort_unstable();
    external.dedup();
    builder.reserve_external_labels(external.len())?;
    for (span, message) in external {
        builder = builder.secondary_external(external_position(span)?, Some(message.into()));
    }
    Ok(Some(builder.finish()))
}

fn joined_reasons(evidence: &[UnavailableEvidence<'_>]) -> Result<Box<str>, diagnostic::Error> {
    let mut bytes = 0_usize;
    let mut previous = None;
    for retained in evidence {
        if previous == Some(retained.reason.as_ref()) {
            continue;
        }
        bytes = bytes
            .checked_add(retained.reason.len())
            .and_then(|bytes| bytes.checked_add(usize::from(previous.is_some()) * 2))
            .ok_or(diagnostic::Error::Allocation)?;
        previous = Some(retained.reason.as_ref());
    }
    let mut joined = String::new();
    joined
        .try_reserve_exact(bytes)
        .map_err(|_| diagnostic::Error::Allocation)?;
    previous = None;
    for retained in evidence {
        if previous == Some(retained.reason.as_ref()) {
            continue;
        }
        if previous.is_some() {
            joined.push_str("; ");
        }
        joined.push_str(&retained.reason);
        previous = Some(retained.reason.as_ref());
    }
    Ok(joined.into_boxed_str())
}

struct UnavailableEvidence<'a> {
    reason: Box<str>,
    source: &'a rawr_import::api::model::Span,
    declaration: &'a rawr_import::api::model::Span,
}

fn unavailable_evidence<'contract>(
    outcome: rawr_import::classify::Outcome<'contract>,
) -> Option<UnavailableEvidence<'contract>> {
    use rawr_import::classify::representability::Representability as R;

    let evidence = match outcome.representability() {
        R::Excluded(reason) => UnavailableEvidence {
            reason: reason.reason().into(),
            source: reason.source_span(),
            declaration: reason.policy_span(),
        },
        R::Rejected(reason) => UnavailableEvidence {
            reason: format!("unsupported Rust declaration ({:?})", reason.kind()).into_boxed_str(),
            source: reason.span(),
            declaration: reason.span(),
        },
        R::Value(_) => UnavailableEvidence {
            reason: "type aliases are deferred until concrete substitution proof exists".into(),
            source: outcome.origin()?.source(),
            declaration: outcome.origin()?.declaration(),
        },
        R::FunctionItem(plan) if callable_generic(plan) => return None,
        R::FunctionItem(_) => UnavailableEvidence {
            reason: "a generic function with a const or closure parameter is not supported in Rawr yet"
                .into(),
            source: outcome.origin()?.source(),
            declaration: outcome.origin()?.declaration(),
        },
        R::Invocation(_) => return None,
        R::Struct(plan) if plan.declaration_only() => UnavailableEvidence {
            reason:
                "generic struct construction is deferred until concrete substitution proof exists"
                    .into(),
            source: outcome.origin()?.source(),
            declaration: outcome.origin()?.declaration(),
        },
        R::Enum(plan) if plan.declaration_only() => UnavailableEvidence {
            reason:
                "generic enum construction is deferred until concrete substitution proof exists"
                    .into(),
            source: outcome.origin()?.source(),
            declaration: outcome.origin()?.declaration(),
        },
        R::Struct(_) | R::Enum(_) | R::Function(_) | R::Method(_) | R::Constant(_) => return None,
    };
    Some(evidence)
}

fn external_position(
    span: &rawr_import::api::model::Span,
) -> Result<diagnostic::ExternalPosition, diagnostic::Error> {
    let bytes = span.byte_range();
    let (start, end) = span.positions();
    diagnostic::ExternalPosition::checked(span.file(), bytes.start, bytes.end, start, end)
}

#[derive(Clone, Copy)]
enum ImportedNamespace {
    Type,
    Value,
    Either,
}

impl ImportedNamespace {
    fn accepts(self, representability: &Representability) -> bool {
        match self {
            Self::Type => matches!(
                representability,
                Representability::Value(_)
                    | Representability::Struct(_)
                    | Representability::Enum(_)
            ),
            Self::Value => matches!(
                representability,
                Representability::Function(_)
                    | Representability::Method(_)
                    | Representability::Constant(_)
                    | Representability::FunctionItem(_)
            ),
            Self::Either => true,
        }
    }
}

/// A Rust path that resolution missed.
enum Miss {
    /// The path may resolve once demanded, and whether it lies below a
    /// wholesale crate re-export (`pub use other_crate;`), which
    /// extraction names without walking. A path below a dependency extracted
    /// from demand may resolve once demanded too; the caller
    /// knows which those are.
    Demand(String, bool),
    /// The dependency's name index has no such path: a Rust-style
    /// unresolved import, never retried.
    Unresolved(String),
}

fn rust_miss(
    contract: &rawr_import::Contract<rawr_import::Classified>,
    error: &crate::resolve::Error,
) -> Option<Miss> {
    use crate::resolve::Error as R;
    use rawr_import::api::ledger::Lookup;
    let path = match error {
        R::UnknownImport { path, .. } => path.to_string(),
        R::UnknownType { name, .. } => name.to_string(),
        R::UnknownMember { owner, member, .. } => format!("{owner}::{member}"),
        _ => return None,
    };
    if !path.contains("::") {
        return None;
    }
    let through_alias = contract.reaches_through_crate_alias(&path);
    // A member missing from a resolved owner may be one a trait the type
    // implements provides. A demanded type brings its own impl tables, and
    // demanding `Type::member` brings the member of each trait it
    // implements. Demanded once, a member still missing is a real miss.
    if matches!(error, R::UnknownMember { .. }) {
        return Some(Miss::Demand(path, through_alias));
    }
    match contract.rust_lookup(&path) {
        Some(Lookup::Missing {
            parent,
            missing,
            suggestion,
        }) => {
            let mut message = format!("unresolved import `{path}`: no `{missing}` in `{parent}`");
            if let Some(suggestion) = suggestion {
                message.push_str(&format!("; did you mean `{suggestion}`?"));
            }
            Some(Miss::Unresolved(message))
        }
        Some(Lookup::ThroughCrate) => Some(Miss::Demand(path, true)),
        _ => Some(Miss::Demand(path, through_alias)),
    }
}

fn diagnostic_path(error: &crate::resolve::Error) -> Option<(String, ImportedNamespace)> {
    use crate::resolve::Error as R;
    match error {
        R::InvalidTypeArguments { ty, .. } => Some((ty.to_string(), ImportedNamespace::Type)),
        R::UnknownMember { owner, .. }
        | R::InvalidConstruction { owner, .. }
        | R::MissingCapability { owner, .. }
        | R::InvalidMatch { owner, .. } => Some((owner.to_string(), ImportedNamespace::Type)),
        R::InvalidHostOperation { item, .. } => Some((item.to_string(), ImportedNamespace::Either)),
        R::InvalidMethodCall { owner, method, .. }
        | R::InvalidMethodSignature { owner, method, .. } => {
            Some((format!("{owner}::{method}"), ImportedNamespace::Value))
        }
        R::TypeMismatch {
            expected, actual, ..
        } => Some((
            if actual.contains("::") {
                actual.to_string()
            } else {
                expected.to_string()
            },
            ImportedNamespace::Type,
        )),
        R::DuplicateItem { .. }
        | R::UnknownType { .. }
        | R::UnknownFunction { .. }
        | R::UnknownVariable { .. }
        | R::MovedValue { .. }
        | R::UnknownImport { .. }
        | R::InvalidImport { .. }
        | R::LoweringUnavailable { .. }
        | R::InvalidDerive { .. }
        | R::DuplicateMember { .. }
        | R::ImmutableBinding { .. }
        | R::InvalidImplOwner { .. }
        | R::DuplicateMethod { .. }
        | R::UnusedValue { .. }
        | R::SelfOutsideImpl { .. }
        | R::ImmutableReceiver { .. }
        | R::UnsupportedNominalSyntax { .. }
        | R::Application(_)
        | R::InvalidPreview { .. }
        | R::InvalidWidget { .. } => None,
    }
}

fn imported_lowering_report(
    evidence: &crate::resolve::LoweringEvidence,
    error: &crate::ParseError,
    sources: &[diagnostic::Source],
    paths: &ImportedPaths<'_>,
) -> Result<Option<diagnostic::Report>, diagnostic::Error> {
    let crate::ParseErrorType::Resolution(resolution) = error.err_type() else {
        return Ok(None);
    };
    let Some((path, namespace)) = diagnostic_path(resolution) else {
        return Ok(None);
    };
    let outcomes = diagnostic_outcomes(paths, &path, namespace)?;
    if outcomes.is_empty() {
        return Ok(None);
    }
    let report = match evidence {
        crate::resolve::LoweringEvidence::Original(source) => {
            let source = sources
                .get(source.index())
                .ok_or(diagnostic::Error::EmptySourceIdentity)?;
            diagnostic::Report::from_parse(source.clone(), error)?
        }
        crate::resolve::LoweringEvidence::Generated { source, span } => {
            let source = sources
                .get(source.index())
                .ok_or(diagnostic::Error::EmptySourceIdentity)?;
            diagnostic::Report::from_parse_at(source.clone(), error, *span)?
        }
        crate::resolve::LoweringEvidence::Unmapped(_) => return Ok(None),
    };
    augment_external(report, outcomes)
}

fn augment_external(
    report: diagnostic::Report,
    outcomes: Vec<rawr_import::classify::Outcome<'_>>,
) -> Result<Option<diagnostic::Report>, diagnostic::Error> {
    let mut spans = Vec::new();
    let span_capacity = outcomes
        .len()
        .checked_mul(2)
        .ok_or(diagnostic::Error::Allocation)?;
    spans
        .try_reserve_exact(span_capacity)
        .map_err(|_| diagnostic::Error::Allocation)?;
    for outcome in outcomes {
        if let Some(origin) = outcome.origin() {
            spans.push(origin.source());
            spans.push(origin.declaration());
        }
    }
    spans.sort_unstable();
    spans.dedup();
    if spans.is_empty() {
        return Ok(None);
    }

    let mut builder = diagnostic::Report::error(report.code(), report.message());
    for source in report.sources() {
        builder = builder.source(source.clone());
    }
    for label in report.labels() {
        let message = label.message().map(ImmutableString::from);
        builder = match label.kind() {
            diagnostic::LabelKind::Primary => {
                builder.primary(label.source().clone(), label.span(), message)?
            }
            diagnostic::LabelKind::Secondary => {
                builder.secondary(label.source().clone(), label.span(), message)?
            }
        };
    }
    for label in report.external_labels() {
        builder = builder.secondary_external(
            label.position().clone(),
            label.message().map(ImmutableString::from),
        );
    }
    if let Some(help) = report.help() {
        builder = builder.help(help);
    }
    for note in report.notes() {
        builder = builder.note(note);
    }
    builder.reserve_external_labels(spans.len())?;
    for span in spans {
        builder = builder.secondary_external(
            external_position(span)?,
            Some("relevant Rust declaration is retained here".into()),
        );
    }
    Ok(Some(builder.finish()))
}

fn diagnostic_outcomes<'contract>(
    paths: &'contract ImportedPaths<'contract>,
    path: &str,
    namespace: ImportedNamespace,
) -> Result<Vec<rawr_import::classify::Outcome<'contract>>, diagnostic::Error> {
    let retained = paths.outcomes(path);
    let mut outcomes = Vec::new();
    outcomes
        .try_reserve_exact(retained.len())
        .map_err(|_| diagnostic::Error::Allocation)?;
    outcomes.extend(retained.filter(|outcome| namespace.accepts(outcome.representability())));
    Ok(outcomes)
}

struct ImportedPaths<'contract> {
    entries: Vec<ImportedPath<'contract>>,
}

struct ImportedPath<'contract> {
    path: Box<str>,
    outcome: rawr_import::classify::Outcome<'contract>,
}

impl<'contract> ImportedPaths<'contract> {
    fn outcomes(
        &self,
        path: &str,
    ) -> impl ExactSizeIterator<Item = rawr_import::classify::Outcome<'contract>> + '_ {
        let start = self
            .entries
            .partition_point(|entry| entry.path.as_ref() < path);
        let end = self
            .entries
            .partition_point(|entry| entry.path.as_ref() <= path);
        self.entries[start..end].iter().map(|entry| entry.outcome)
    }
}

fn imported_paths<'contract>(
    contract: &'contract rawr_import::Contract<rawr_import::Classified>,
    allocation: &mut rawr_import::contract::Allocation,
) -> Result<ImportedPaths<'contract>, Error> {
    let mut capacity = contract.outcomes().len();
    for outcome in contract.outcomes() {
        let associated = outcome
            .implementations()
            .map(rawr_import::classify::ImplementationOutcome::associated)
            .map(|associated| associated.len())
            .try_fold(0_usize, usize::checked_add)
            .ok_or(Error::Allocation)?;
        capacity = capacity.checked_add(associated).ok_or(Error::Allocation)?;
    }
    allocation.retain_checked(
        Some(capacity),
        capacity.checked_mul(std::mem::size_of::<ImportedPath<'_>>()),
    )?;
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(capacity)
        .map_err(|_| Error::Allocation)?;
    for outcome in contract.outcomes() {
        let path = import_path(outcome.export(), allocation)?;
        for associated in outcome
            .implementations()
            .flat_map(rawr_import::classify::ImplementationOutcome::associated)
        {
            let associated_bytes = path
                .len()
                .checked_add(2)
                .and_then(|bytes| bytes.checked_add(associated.name().len()))
                .ok_or(Error::Allocation)?;
            allocation.retain(1, associated_bytes)?;
            let mut associated_path = String::new();
            associated_path
                .try_reserve_exact(associated_bytes)
                .map_err(|_| Error::Allocation)?;
            associated_path.push_str(&path);
            associated_path.push_str("::");
            associated_path.push_str(associated.name());
            entries.push(ImportedPath {
                path: associated_path.into_boxed_str(),
                outcome: associated,
            });
        }
        entries.push(ImportedPath {
            path: path.into_boxed_str(),
            outcome,
        });
    }
    if entries.len() != capacity {
        return Err(Error::Cardinality);
    }
    entries.sort_unstable_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.outcome.identity().cmp(right.outcome.identity()))
    });
    Ok(ImportedPaths { entries })
}

fn import_path(
    key: &rawr_import::api::identity::Export,
    allocation: &mut rawr_import::contract::Allocation,
) -> Result<String, Error> {
    let bytes = key
        .path()
        .try_fold(key.root_alias().len(), |bytes, segment| {
            bytes.checked_add(2)?.checked_add(segment.len())
        });
    let bytes = bytes.ok_or(Error::Allocation)?;
    allocation.retain(1, bytes)?;
    let mut path = String::new();
    path.try_reserve_exact(bytes)
        .map_err(|_| Error::Allocation)?;
    path.push_str(key.root_alias());
    for segment in key.path() {
        path.push_str("::");
        path.push_str(segment);
    }
    Ok(path)
}

struct Facade<'contract> {
    root: &'contract str,
    entries: Vec<Entry<'contract>>,
    /// The traits the program names: modules, as in the facade plan.
    traits: Vec<Box<str>>,
}

struct Entry<'contract> {
    relative: Box<str>,
    definition: rawr_import::surface::identity::Definition,
    outcome: rawr_import::classify::Outcome<'contract>,
}

struct Types;

fn relative_path(
    key: &rawr_import::api::identity::Export,
    allocation: &mut rawr_import::contract::Allocation,
) -> Result<String, Error> {
    let mut segments = key.path();
    let count = segments.len();
    let bytes = segments.try_fold(0_usize, |bytes, segment| {
        bytes
            .checked_add(segment.len())?
            .checked_add(if bytes == 0 { 0 } else { 2 })
    });
    let bytes = bytes.ok_or(Error::Allocation)?;
    allocation.retain(1, bytes)?;
    let mut relative = String::new();
    relative
        .try_reserve_exact(bytes)
        .map_err(|_| Error::Allocation)?;
    for (index, segment) in key.path().enumerate() {
        if index != 0 {
            relative.push_str("::");
        }
        relative.push_str(segment);
    }
    if count == 0 && !relative.is_empty() {
        return Err(Error::Cardinality);
    }
    Ok(relative)
}

fn shape_usage(representability: &Representability) -> Result<(usize, usize), Error> {
    match representability {
        Representability::Struct(plan) => {
            let constructor = plan
                .constructor()
                .map_or(
                    Some(0),
                    rawr_import::surface::identity::Selector::retained_bytes,
                )
                .ok_or(Error::Allocation)?;
            let base = std::mem::size_of::<host::type_def::Shape>()
                .checked_add(constructor)
                .ok_or(Error::Allocation)?;
            let bytes = plan
                .fields()
                .iter()
                .try_fold(base, |bytes, field| {
                    bytes
                        .checked_add(std::mem::size_of::<host::type_def::Field>())?
                        .checked_add(field.name().len())?
                        .checked_add(field.selector().retained_bytes()?)
                })
                .ok_or(Error::Allocation)?;
            Ok((
                plan.fields()
                    .len()
                    .checked_add(1)
                    .ok_or(Error::Allocation)?,
                bytes,
            ))
        }
        Representability::Enum(plan) => {
            let (items, bytes) = plan
                .variants()
                .iter()
                .try_fold(
                    (1_usize, std::mem::size_of::<host::type_def::Shape>()),
                    |(items, bytes), variant| {
                        variant.fields().iter().try_fold(
                            (
                                items.checked_add(1)?,
                                bytes
                                    .checked_add(std::mem::size_of::<host::type_def::Variant>())?
                                    .checked_add(variant.name().len())?
                                    .checked_add(variant.selector().retained_bytes()?)?,
                            ),
                            |(items, bytes), field| {
                                Some((
                                    items.checked_add(1)?,
                                    bytes
                                        .checked_add(std::mem::size_of::<host::type_def::Field>())?
                                        .checked_add(field.name().len())?
                                        .checked_add(field.selector().retained_bytes()?)?,
                                ))
                            },
                        )
                    },
                )
                .ok_or(Error::Allocation)?;
            Ok((items, bytes))
        }
        Representability::Value(_)
        | Representability::Function(_)
        | Representability::Method(_)
        | Representability::Constant(_)
        | Representability::FunctionItem(_)
        | Representability::Invocation(_)
        | Representability::Excluded(_)
        | Representability::Rejected(_) => Ok((0, 0)),
    }
}

fn install(
    contract: &rawr_import::Contract<rawr_import::Classified>,
    registry: &mut host::Registry,
    allocation: &mut rawr_import::contract::Allocation,
) -> Result<(), Error> {
    let retained = contract
        .outcomes()
        .filter(|outcome| {
            !matches!(
                outcome.representability(),
                Representability::Excluded(_) | Representability::Rejected(_)
            )
        })
        .count();
    let facade_count = contract
        .outcomes()
        .enumerate()
        .filter(|(index, outcome)| {
            !matches!(
                outcome.representability(),
                Representability::Excluded(_) | Representability::Rejected(_)
            ) && !contract.outcomes().take(*index).any(|prior| {
                !matches!(
                    prior.representability(),
                    Representability::Excluded(_) | Representability::Rejected(_)
                ) && prior.export().root_alias() == outcome.export().root_alias()
            })
        })
        .count();
    allocation.retain_checked(
        retained.checked_add(facade_count),
        retained
            .checked_mul(std::mem::size_of::<Entry<'_>>())
            .and_then(|bytes| {
                bytes.checked_add(facade_count.checked_mul(std::mem::size_of::<Facade<'_>>())?)
            }),
    )?;
    let mut facades = Vec::<Facade<'_>>::new();
    facades
        .try_reserve_exact(facade_count)
        .map_err(|_| Error::Allocation)?;
    for outcome in contract.outcomes() {
        let named_trait = matches!(
            outcome.representability(),
            Representability::Rejected(reason)
                if reason.kind() == representability::reject::Kind::Trait
        );
        if named_trait {
            let root = outcome.export().root_alias();
            let relative = relative_path(outcome.export(), allocation)?;
            let index = match facades.binary_search_by(|facade| facade.root.cmp(root)) {
                Ok(index) => index,
                Err(index) => {
                    facades.try_reserve(1).map_err(|_| Error::Allocation)?;
                    facades.insert(
                        index,
                        Facade {
                            root,
                            entries: Vec::new(),
                            traits: Vec::new(),
                        },
                    );
                    index
                }
            };
            let traits = &mut facades[index].traits;
            if let Err(position) = traits.binary_search_by(|path| path.as_ref().cmp(&relative)) {
                traits.try_reserve(1).map_err(|_| Error::Allocation)?;
                traits.insert(position, relative.into_boxed_str());
            }
            continue;
        }
        if matches!(
            outcome.representability(),
            Representability::Excluded(_) | Representability::Rejected(_)
        ) {
            continue;
        }
        let definition = outcome.identity().definition().ok_or(Error::Cardinality)?;
        let key = outcome.export();
        let root = key.root_alias();
        let relative = relative_path(key, allocation)?;
        if relative.is_empty() {
            return Err(Error::InvalidPath { path: root.into() });
        }
        let facade_index = match facades.binary_search_by(|facade| facade.root.cmp(root)) {
            Ok(index) => index,
            Err(index) => {
                facades.insert(
                    index,
                    Facade {
                        root,
                        entries: Vec::new(),
                        traits: Vec::new(),
                    },
                );
                index
            }
        };
        let facade = &mut facades[facade_index];
        facade
            .entries
            .try_reserve(1)
            .map_err(|_| Error::Allocation)?;
        facade.entries.push(Entry {
            relative: relative.into_boxed_str(),
            definition,
            outcome,
        });
    }

    let shape_count = facades
        .iter()
        .flat_map(|facade| &facade.entries)
        .enumerate()
        .filter(|(index, entry)| {
            matches!(
                entry.outcome.representability(),
                Representability::Struct(_) | Representability::Enum(_)
            ) && !facades
                .iter()
                .flat_map(|facade| &facade.entries)
                .take(*index)
                .any(|prior| prior.definition == entry.definition)
        })
        .count();
    allocation.retain_checked(
        Some(shape_count),
        shape_count.checked_mul(std::mem::size_of::<(
            rawr_import::surface::identity::Definition,
            crate::Shared<host::type_def::Shape>,
        )>()),
    )?;
    let mut shapes = Vec::new();
    shapes
        .try_reserve_exact(shape_count)
        .map_err(|_| Error::Allocation)?;
    // An item whose signature names a type its own facade does not declare
    // (one the importer could not model, or one only another crate exports)
    // is unavailable by itself, as in the facade plan the runner builds. A
    // facade resolves only the types it declares.
    for facade in facades {
        let declared = facade
            .entries
            .iter()
            .filter(|entry| {
                matches!(
                    entry.outcome.representability(),
                    Representability::Struct(_) | Representability::Enum(_)
                )
            })
            .map(|entry| entry.definition)
            .collect::<std::collections::BTreeSet<_>>();
        let definition = build(contract, facade, &declared, &mut shapes, allocation)?;
        let (items, bytes) = definition.registry_usage();
        allocation.retain_checked(items, bytes)?;
        registry.install(definition)?;
    }
    Ok(())
}

fn build(
    _contract: &rawr_import::Contract<rawr_import::Classified>,
    mut facade: Facade<'_>,
    declared: &std::collections::BTreeSet<rawr_import::surface::identity::Definition>,
    shapes: &mut Vec<(
        rawr_import::surface::identity::Definition,
        crate::Shared<host::type_def::Shape>,
    )>,
    allocation: &mut rawr_import::contract::Allocation,
) -> Result<host::facade::Definition, Error> {
    facade.entries.sort_unstable_by(|left, right| {
        left.relative
            .cmp(&right.relative)
            .then_with(|| left.outcome.identity().cmp(right.outcome.identity()))
    });
    let type_count = facade
        .entries
        .iter()
        .enumerate()
        .filter(|(index, entry)| {
            matches!(
                entry.outcome.representability(),
                Representability::Struct(_) | Representability::Enum(_)
            ) && !facade.entries[..*index]
                .iter()
                .any(|prior| prior.definition == entry.definition)
        })
        .count();
    allocation.retain_checked(
        Some(type_count),
        type_count.checked_mul(std::mem::size_of::<(
            rawr_import::surface::identity::Definition,
            usize,
        )>()),
    )?;
    let mut type_entries = Vec::new();
    type_entries
        .try_reserve_exact(type_count)
        .map_err(|_| Error::Allocation)?;
    for (index, entry) in facade.entries.iter().enumerate() {
        if !matches!(
            entry.outcome.representability(),
            Representability::Struct(_) | Representability::Enum(_)
        ) {
            continue;
        }
        let entry_index = match type_entries
            .binary_search_by_key(&entry.definition, |(definition, _)| *definition)
        {
            Ok(_) => continue,
            Err(entry_index) => entry_index,
        };
        type_entries.insert(entry_index, (entry.definition, index));
    }
    let types = Types;
    let abi = host::facade::Abi::new(1)?;
    let mut builder = host::facade::Builder::new_imported(facade.root, abi, allocation)?;
    let mut modules = Vec::new();
    for entry in &facade.entries {
        let is_type = matches!(
            entry.outcome.representability(),
            Representability::Struct(_) | Representability::Enum(_)
        );
        collect_modules(
            &entry.relative,
            is_type,
            &facade.entries,
            &type_entries,
            &mut modules,
            allocation,
        )?;
    }
    for module in modules {
        builder.imported_module(&module, allocation)?;
    }
    // A trait the program names is a module (`use rand::Rng;`), unless a
    // type already has its path.
    for path in &facade.traits {
        let prefixes = path.match_indices("::").map(|(end, _)| &path[..end]);
        for module in prefixes.chain(std::iter::once(path.as_ref())) {
            match builder.imported_module(module, allocation) {
                Ok(_) | Err(host::Error::DuplicateFacadeEntry { .. }) => {}
                Err(error) => return Err(error.into()),
            }
        }
    }

    for entry in &facade.entries {
        let (parameters, defaults, copy, default, debug, clone) = match entry
            .outcome
            .representability()
        {
            Representability::Struct(plan) => (
                names(plan.parameters(), allocation)?,
                defaults(&types, plan.parameters(), allocation)?,
                plan.copy(),
                plan.default(),
                plan.debug(),
                plan.cloneable(),
            ),
            Representability::Enum(plan) => (
                names(plan.parameters(), allocation)?,
                defaults(&types, plan.parameters(), allocation)?,
                plan.copy(),
                plan.default(),
                plan.debug(),
                plan.cloneable(),
            ),
            Representability::Value(_)
            | Representability::Function(_)
            | Representability::Method(_)
            | Representability::Constant(_)
            | Representability::FunctionItem(_)
            | Representability::Invocation(_)
            | Representability::Excluded(_)
            | Representability::Rejected(_) => continue,
        };
        let shape_index = shapes.binary_search_by_key(&entry.definition, |(identity, _)| *identity);
        let shape = match shape_index {
            Ok(index) => shapes[index].1.clone(),
            Err(index) => {
                let (items, bytes) = shape_usage(entry.outcome.representability())?;
                allocation.retain(items, bytes)?;
                let new_shape = match entry.outcome.representability() {
                    Representability::Struct(plan) => host::type_def::Shape::Struct {
                        form: match plan.form() {
                            rawr_import::api::model::FieldForm::Unit => host::type_def::Form::Unit,
                            rawr_import::api::model::FieldForm::Tuple => {
                                host::type_def::Form::Tuple
                            }
                            rawr_import::api::model::FieldForm::Record => {
                                host::type_def::Form::Record
                            }
                        },
                        constructor: plan
                            .constructor()
                            .map(rawr_import::surface::identity::Selector::try_clone)
                            .transpose()?,
                        fields: {
                            let mut fields = Vec::new();
                            fields
                                .try_reserve_exact(plan.fields().len())
                                .map_err(|_| Error::Allocation)?;
                            for field in plan.fields() {
                                fields.push(host::type_def::Field::new(
                                    field.name().into(),
                                    types.convert(field.ty(), plan.parameters(), allocation)?,
                                    field.selector().try_clone()?,
                                ));
                            }
                            fields.into_boxed_slice()
                        },
                        constructible: plan.constructible(),
                    },
                    Representability::Enum(plan) => {
                        let mut variants = Vec::new();
                        variants
                            .try_reserve_exact(plan.variants().len())
                            .map_err(|_| Error::Allocation)?;
                        for variant in plan.variants() {
                            let mut fields = Vec::new();
                            fields
                                .try_reserve_exact(variant.fields().len())
                                .map_err(|_| Error::Allocation)?;
                            for field in variant.fields() {
                                fields.push(host::type_def::Field::new(
                                    field.name().into(),
                                    types.convert(field.ty(), plan.parameters(), allocation)?,
                                    field.selector().try_clone()?,
                                ));
                            }
                            variants.push(host::type_def::Variant::new(
                                variant.name().into(),
                                fields.into_boxed_slice(),
                                variant.constructible(),
                                variant.selector().try_clone()?,
                            ));
                        }
                        host::type_def::Shape::Enum(variants.into_boxed_slice())
                    }
                    Representability::Value(_)
                    | Representability::Function(_)
                    | Representability::Method(_)
                    | Representability::Constant(_)
                    | Representability::FunctionItem(_)
                    | Representability::Invocation(_)
                    | Representability::Excluded(_)
                    | Representability::Rejected(_) => return Err(Error::Cardinality),
                };
                let shape = crate::Shared::new(new_shape);
                shapes.insert(index, (entry.definition, shape.clone()));
                shape
            }
        };
        let declaration_bytes = entry.outcome.identity().selector().retained_bytes();
        let definition_bytes = declaration_bytes.and_then(|selector| {
            std::mem::size_of::<host::type_def::Definition>().checked_add(selector)
        });
        allocation.retain_checked(Some(1), definition_bytes)?;
        builder.imported_type(
            entry.relative.as_ref(),
            imported_identity(entry.definition),
            copy,
            default,
            debug,
            clone,
            parameters,
            defaults,
            shape,
            allocation,
        )?;
    }

    for entry in &facade.entries {
        let item = imported_item(&types, entry.relative.as_ref(), entry.outcome, allocation)?;
        if let Some(item) = item.filter(|_| names_declared(entry.outcome, declared)) {
            builder.item(item)?;
        }
        for associated in entry
            .outcome
            .implementations()
            .flat_map(rawr_import::classify::ImplementationOutcome::associated)
        {
            let path_bytes = entry
                .relative
                .len()
                .checked_add(2)
                .and_then(|bytes| bytes.checked_add(associated.name().len()))
                .ok_or(Error::Allocation)?;
            allocation.retain(1, path_bytes)?;
            let mut path = String::new();
            path.try_reserve_exact(path_bytes)
                .map_err(|_| Error::Allocation)?;
            path.push_str(&entry.relative);
            path.push_str("::");
            path.push_str(associated.name());
            if let Some(item) = imported_item(&types, &path, associated, allocation)?
                .filter(|_| names_declared(associated, declared))
            {
                builder.item(item)?;
            }
        }
    }
    builder.finish_imported(allocation).map_err(Error::from)
}

fn imported_item(
    types: &Types,
    path: &str,
    outcome: rawr_import::classify::Outcome<'_>,
    allocation: &mut rawr_import::contract::Allocation,
) -> Result<Option<host::item::Definition>, Error> {
    use rawr_import::surface::identity::Selector;

    let retains_item = match outcome.representability() {
        Representability::Function(_)
        | Representability::Method(_)
        | Representability::Constant(_)
        | Representability::Invocation(_) => true,
        Representability::FunctionItem(plan) => callable_generic(plan),
        _ => false,
    };
    if retains_item {
        let selector_bytes = outcome.identity().selector().retained_bytes();
        let bytes = selector_bytes.and_then(|selector| {
            std::mem::size_of::<host::item::Definition>()
                .checked_add(unqualified_path_bytes(path)?)?
                .checked_add(selector)
        });
        allocation.retain_checked(Some(1), bytes)?;
    }
    let item = match outcome.representability() {
        Representability::Function(plan) => Some(
            match outcome.identity().selector() {
                Selector::AssociatedFunction { .. } => host::item::Definition::associated_function(
                    path,
                    signature(types, plan.signature(), allocation)?,
                )?,
                Selector::Function { .. } => host::item::Definition::function(
                    path,
                    signature(types, plan.signature(), allocation)?,
                )?,
                _ => return Err(Error::Cardinality),
            }
            .imported(outcome.identity().selector().try_clone()?),
        ),
        Representability::Method(plan) => Some(
            host::item::Definition::method(
                path,
                receiver(plan.receiver()),
                signature(types, plan.signature(), allocation)?,
            )?
            .imported(outcome.identity().selector().try_clone()?),
        ),
        Representability::Constant(plan) => Some(
            match outcome.identity().selector() {
                Selector::AssociatedConstant { .. } => host::item::Definition::associated_value(
                    path,
                    types.convert(plan.ty(), &[], allocation)?,
                )?,
                Selector::Constant { .. } => {
                    host::item::Definition::value(path, types.convert(plan.ty(), &[], allocation)?)?
                }
                _ => return Err(Error::Cardinality),
            }
            .imported(outcome.identity().selector().try_clone()?),
        ),
        Representability::Invocation(plan) => Some(
            match outcome.identity().selector() {
                Selector::AssociatedFunction { .. } => host::item::Definition::associated_function(
                    path,
                    signature(types, plan.signature(), allocation)?,
                )?,
                Selector::Method { .. } => host::item::Definition::method(
                    path,
                    receiver(plan.receiver().ok_or(Error::Cardinality)?),
                    signature(types, plan.signature(), allocation)?,
                )?,
                _ => return Err(Error::Cardinality),
            }
            .imported(outcome.identity().selector().try_clone()?),
        ),
        // A generic function Rawr calls at the types it infers or a
        // turbofish names. The bridge calls it at those types, where rustc
        // checks its bounds.
        Representability::FunctionItem(plan) if callable_generic(plan) => Some(
            match outcome.identity().selector() {
                Selector::AssociatedFunction { .. } => host::item::Definition::associated_function(
                    path,
                    signature(types, plan.signature(), allocation)?,
                )?,
                Selector::Function { .. } => host::item::Definition::function(
                    path,
                    signature(types, plan.signature(), allocation)?,
                )?,
                Selector::Method { .. } => host::item::Definition::method(
                    path,
                    receiver(plan.receiver().ok_or(Error::Cardinality)?),
                    signature(types, plan.signature(), allocation)?,
                )?,
                _ => return Err(Error::Cardinality),
            }
            .imported(outcome.identity().selector().try_clone()?),
        ),
        Representability::Value(_)
        | Representability::Struct(_)
        | Representability::Enum(_)
        | Representability::FunctionItem(_)
        | Representability::Excluded(_)
        | Representability::Rejected(_) => None,
    };
    Ok(item)
}

/// Whether every imported type the outcome's signature names is declared.
fn names_declared(
    outcome: rawr_import::classify::Outcome<'_>,
    declared: &std::collections::BTreeSet<rawr_import::surface::identity::Definition>,
) -> bool {
    let signature = match outcome.representability() {
        Representability::Function(plan) => plan.signature(),
        Representability::Method(plan) => plan.signature(),
        Representability::FunctionItem(plan) => plan.signature(),
        Representability::Invocation(plan) => plan.signature(),
        Representability::Constant(plan) => return ty_ok(plan.ty(), declared),
        _ => return true,
    };
    signature.inputs().iter().all(|input| ty_ok(input, declared))
        && ty_ok(signature.output(), declared)
}

/// Whether every imported type `ty` names is declared.
fn ty_ok(
    ty: &representability::convert::Type,
    declared: &std::collections::BTreeSet<rawr_import::surface::identity::Definition>,
) -> bool {
    use representability::convert::Type;
    match ty {
        Type::Nominal {
            definition,
            arguments,
        } => {
            (declared.contains(definition)
                || rawr_import::surface::identity::Definition::is_projected(definition.as_bytes()))
                && arguments.iter().all(|ty| ty_ok(ty, declared))
        }
        Type::FunctionItem { input, output } | Type::Result(input, output) => {
            ty_ok(input, declared) && ty_ok(output, declared)
        }
        Type::Option(element) => ty_ok(element, declared),
        Type::ProfileMessage { payload, .. } => ty_ok(payload, declared),
        Type::Unit | Type::Builtin(_) | Type::Parameter(_) => true,
    }
}

/// A generic item Rawr calls directly: every parameter a type, none a
/// callable (those are function-item operands).
fn callable_generic(plan: &representability::callable::Item) -> bool {
    plan.class().is_none()
        && plan
            .signature()
            .parameters()
            .iter()
            .all(|parameter| parameter.kind() == representability::signature::Kind::Type)
}

fn unqualified_path_bytes(path: &str) -> Option<usize> {
    let segments = path.split("::").count();
    let strings = path
        .split("::")
        .try_fold(0_usize, |bytes, segment| bytes.checked_add(segment.len()))?;
    segments
        .checked_mul(std::mem::size_of::<ImmutableString>())?
        .checked_add(strings)
}

impl Types {
    fn convert(
        &self,
        ty: &representability::convert::Type,
        parameters: &[representability::signature::Parameter],
        allocation: &mut rawr_import::contract::Allocation,
    ) -> Result<host::type_ref::Type, Error> {
        let (items, bytes) = type_usage(ty, parameters)?;
        allocation.retain(items, bytes)?;
        Self::convert_charged(ty, parameters)
    }

    fn convert_charged(
        ty: &representability::convert::Type,
        parameters: &[representability::signature::Parameter],
    ) -> Result<host::type_ref::Type, Error> {
        use representability::convert::{Builtin, Type};
        Ok(match ty {
            Type::Unit => host::type_ref::Type::unit(),
            Type::Builtin(builtin) => host::type_ref::Type::builtin(match builtin {
                Builtin::Bool => host::type_ref::Builtin::Bool,
                Builtin::Char => host::type_ref::Builtin::Char,
                Builtin::F64 => host::type_ref::Builtin::F64,
                Builtin::I64 => host::type_ref::Builtin::I64,
                Builtin::InclusiveRange => host::type_ref::Builtin::InclusiveRange,
                Builtin::String | Builtin::Str | Builtin::Text => host::type_ref::Builtin::String,
                Builtin::I8 => host::type_ref::Builtin::I8,
                Builtin::I16 => host::type_ref::Builtin::I16,
                Builtin::I32 => host::type_ref::Builtin::I32,
                Builtin::Isize => host::type_ref::Builtin::Isize,
                Builtin::U8 => host::type_ref::Builtin::U8,
                Builtin::U16 => host::type_ref::Builtin::U16,
                Builtin::U32 => host::type_ref::Builtin::U32,
                Builtin::U64 => host::type_ref::Builtin::U64,
                Builtin::Usize => host::type_ref::Builtin::Usize,
                Builtin::F32 => host::type_ref::Builtin::F32,
            }),
            Type::Parameter(index) => {
                let parameter = parameters
                    .iter()
                    .find(|parameter| parameter.index() == *index)
                    .ok_or(Error::Cardinality)?;
                host::type_ref::Type::parameter(parameter.name())?
            }
            Type::Nominal {
                definition,
                arguments,
            } => {
                let mut converted = Vec::new();
                converted
                    .try_reserve_exact(arguments.len())
                    .map_err(|_| Error::Allocation)?;
                for argument in arguments {
                    converted.push(Self::convert_charged(argument, parameters)?);
                }
                host::type_ref::Type::imported_boxed(
                    imported_identity(*definition),
                    converted.into_boxed_slice(),
                )
            }
            Type::FunctionItem { input, output } => host::type_ref::Type::imported_function_item(
                Self::convert_charged(input, parameters)?,
                Self::convert_charged(output, parameters)?,
            ),
            Type::ProfileMessage { payload, .. } => Self::convert_charged(payload, parameters)?,
            Type::Option(element) => {
                host::type_ref::Type::option(Self::convert_charged(element, parameters)?)
            }
            Type::Result(ok, err) => host::type_ref::Type::result(
                Self::convert_charged(ok, parameters)?,
                Self::convert_charged(err, parameters)?,
            ),
        })
    }
}

fn signature(
    types: &Types,
    signature: &representability::signature::Signature,
    allocation: &mut rawr_import::contract::Allocation,
) -> Result<host::signature::Definition, Error> {
    let bytes = signature.parameters().iter().try_fold(
        std::mem::size_of::<host::signature::Definition>(),
        |bytes, parameter| bytes.checked_add(parameter.name().len()),
    );
    let items = signature
        .parameters()
        .len()
        .checked_add(signature.inputs().len())
        .and_then(|items| items.checked_add(1));
    allocation.retain_checked(items, bytes)?;
    let mut builder = host::signature::Builder::new();
    for parameter in signature.parameters() {
        builder.type_parameter(parameter.name(), &[])?;
    }
    for input in signature.inputs() {
        builder.try_parameter(types.convert(input, signature.parameters(), allocation)?)?;
    }
    builder.output(types.convert(signature.output(), signature.parameters(), allocation)?);
    Ok(builder.finish())
}

fn type_usage(
    ty: &representability::convert::Type,
    parameters: &[representability::signature::Parameter],
) -> Result<(usize, usize), Error> {
    use representability::convert::Type;

    let (nested_items, nested_bytes) = match ty {
        Type::Parameter(index) => {
            let parameter = parameters
                .iter()
                .find(|parameter| parameter.index() == *index)
                .ok_or(Error::Cardinality)?;
            (0, parameter.name().len())
        }
        Type::Nominal { arguments, .. } => {
            arguments
                .iter()
                .try_fold((0_usize, 0_usize), |(items, bytes), argument| {
                    let (argument_items, argument_bytes) = type_usage(argument, parameters)?;
                    Ok::<_, Error>((
                        items.checked_add(argument_items).ok_or(Error::Allocation)?,
                        bytes.checked_add(argument_bytes).ok_or(Error::Allocation)?,
                    ))
                })?
        }
        Type::FunctionItem { input, output } | Type::Result(input, output) => {
            let (input_items, input_bytes) = type_usage(input, parameters)?;
            let (output_items, output_bytes) = type_usage(output, parameters)?;
            (
                input_items
                    .checked_add(output_items)
                    .ok_or(Error::Allocation)?,
                input_bytes
                    .checked_add(output_bytes)
                    .ok_or(Error::Allocation)?,
            )
        }
        Type::ProfileMessage { payload, .. } | Type::Option(payload) => {
            type_usage(payload, parameters)?
        }
        Type::Unit | Type::Builtin(_) => (0, 0),
    };
    Ok((
        nested_items.checked_add(1).ok_or(Error::Allocation)?,
        nested_bytes
            .checked_add(std::mem::size_of::<host::type_ref::Type>())
            .ok_or(Error::Allocation)?,
    ))
}

fn names(
    parameters: &[representability::signature::Parameter],
    allocation: &mut rawr_import::contract::Allocation,
) -> Result<Box<[ImmutableString]>, Error> {
    let bytes = parameters.iter().try_fold(0_usize, |bytes, parameter| {
        bytes.checked_add(parameter.name().len())
    });
    allocation.retain_checked(Some(parameters.len()), bytes)?;
    let mut names = Vec::new();
    names
        .try_reserve_exact(parameters.len())
        .map_err(|_| Error::Allocation)?;
    for parameter in parameters {
        names.push(parameter.name().into());
    }
    Ok(names.into_boxed_slice())
}

/// Each parameter's default type, or none when no parameter has one.
fn defaults(
    types: &Types,
    parameters: &[representability::signature::Parameter],
    allocation: &mut rawr_import::contract::Allocation,
) -> Result<Box<[Option<host::type_ref::Type>]>, Error> {
    if parameters.iter().all(|parameter| parameter.default().is_none()) {
        return Ok(Box::default());
    }
    let mut defaults = Vec::new();
    defaults
        .try_reserve_exact(parameters.len())
        .map_err(|_| Error::Allocation)?;
    for parameter in parameters {
        defaults.push(
            parameter
                .default()
                .map(|default| types.convert(default, parameters, allocation))
                .transpose()?,
        );
    }
    Ok(defaults.into_boxed_slice())
}

fn receiver(receiver: representability::ownership::Receiver) -> receiver::Mode {
    match receiver {
        representability::ownership::Receiver::Owned => receiver::Mode::Owned,
        representability::ownership::Receiver::Shared => receiver::Mode::Shared,
        representability::ownership::Receiver::Mutable => receiver::Mode::Mutable,
    }
}

fn imported_identity(
    definition: rawr_import::surface::identity::Definition,
) -> host::type_def::import::Identity {
    host::type_def::import::Identity::new(*definition.as_bytes())
}

fn collect_modules(
    path: &str,
    is_type: bool,
    entries: &[Entry<'_>],
    types: &[(rawr_import::surface::identity::Definition, usize)],
    modules: &mut Vec<Box<str>>,
    allocation: &mut rawr_import::contract::Allocation,
) -> Result<(), Error> {
    for (separator, _) in path.match_indices("::") {
        let candidate = &path[..separator];
        if types
            .iter()
            .any(|(_, index)| entries[*index].relative.as_ref() == candidate)
        {
            break;
        }
        let index = match modules.binary_search_by(|module| module.as_ref().cmp(candidate)) {
            Ok(_) => continue,
            Err(index) => index,
        };
        allocation.retain_checked(
            Some(1),
            std::mem::size_of::<Box<str>>().checked_add(candidate.len()),
        )?;
        modules.try_reserve(1).map_err(|_| Error::Allocation)?;
        modules.insert(index, candidate.into());
    }
    if is_type && let Ok(index) = modules.binary_search_by(|module| module.as_ref().cmp(path)) {
        modules.remove(index);
    }
    Ok(())
}
