//! Symbolic imported contract integration for Rawr package compilation.

use rawr_import::classify::representability::{self, Representability};

use crate::{capability, diagnostic, host, receiver, ImmutableString};

#[doc(hidden)]
pub mod runner;

/// Same-pass compilation against metadata owned by the retained native compiler.
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

struct CompiledImported {
    demand: rawr_import::Demand<rawr_import::Sealed, Brand>,
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
    /// A file of the program changed while it was built.
    #[error("a source file changed during the build; run it again")]
    SnapshotMismatch,
    /// The program uses a crate item that the build of its dependencies
    /// couldn't bind. `selector` is for tests and never shown.
    #[error("`{path}` is not supported in Rawr yet")]
    UnmappedImportedUse {
        /// Fully qualified imported path.
        path: Box<str>,
        /// Closed semantic operation selector.
        selector: rawr_import::surface::identity::Selector,
    },
    /// Imported contract lookup evidence could not reserve its exact capacity.
    #[error("out of memory while reading the program's dependencies")]
    Allocation,
    /// A crate's own root named as an item.
    #[error("`{path}` is not supported in Rawr yet")]
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
        self.compile_imported_applied(contract, snapshot, &limits, &[])
            .map(|compiled| compiled.demand)
    }

    /// Compile against a contract with the proof-backed applications its
    /// program's imported calls need (D650). Each was proved locally from
    /// intrinsic facts; a call none answers fails with the call to prove.
    #[doc(hidden)]
    pub fn compile_imported_with_applications(
        &self,
        contract: &rawr_import::Contract<rawr_import::Classified>,
        snapshot: crate::reload::source::Snapshot,
        applications: &[rawr_import::surface::application::Application],
    ) -> Result<rawr_import::Demand<rawr_import::Sealed, Brand>, super::Error> {
        let limits = rawr_import::cargo::Limits::standard();
        self.compile_imported_applied(contract, snapshot, &limits, applications)
            .map(|compiled| compiled.demand)
    }

    fn compile_imported_applied(
        &self,
        contract: &rawr_import::Contract<rawr_import::Classified>,
        snapshot: crate::reload::source::Snapshot,
        limits: &rawr_import::cargo::Limits,
        applications: &[rawr_import::surface::application::Application],
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
        let facade = rawr_import::surface::facade::Plan::encode(contract)
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
        install(contract, &mut registry, &mut allocation).map_err(super::Error::import)?;
        admit_applications(&mut registry, applications).map_err(super::Error::import)?;
        let mut program = match crate::resolve::package::resolve(graph, &registry) {
            Ok(program) => program,
            Err(failure) => {
                if let crate::resolve::Error::ImportedApplicationRequired { call, .. } =
                    &failure.error
                {
                    let call = call.clone();
                    return Err(super::Error::resolution(sources.clone(), failure)
                        .with_rust_application(call));
                }
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
                    Some(Miss::Demand(path, through_alias, member)) => {
                        Some((path, through_alias, member))
                    }
                    None => None,
                };
                let error = super::Error::resolution(sources.clone(), failure);
                return Err(match demand {
                    Some((path, through_alias, member)) => {
                        error.with_rust_demand(&path, through_alias, member)
                    }
                    None => error,
                });
            }
        };
        let symbolic =
            match crate::resolve::lower_imported(self, &mut program, &registry, &mut allocation) {
                Ok(uses) => uses,
                Err(crate::resolve::ImportedFailure::Surface(error)) => {
                    return Err(super::Error::import(Error::Surface(error)));
                }
                Err(crate::resolve::ImportedFailure::Lowering(mut failure)) => {
                    // What the pass missed besides this failure, asked for
                    // with it (#662).
                    let misses = failure
                        .take_unknowns()
                        .iter()
                        .filter_map(|unknown| match rust_miss(contract, unknown) {
                            Some(Miss::Demand(path, through_alias, member)) => {
                                Some((path, through_alias, member))
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    let (evidence, mut error) = failure.into_parts();
                    // A miss a proof may settle is reported and demanded as
                    // the ordinary miss, carrying the call to prove first.
                    let mut proving = None;
                    if let crate::ParseErrorType::Resolution(
                        crate::resolve::Error::ImportedApplicationRequired { call, miss, .. },
                    ) = error.err_type()
                    {
                        let call = call.clone();
                        match miss {
                            None => {
                                return Err(super::Error::compilation(
                                    sources.clone(),
                                    evidence,
                                    error,
                                )
                                .with_rust_application(call));
                            }
                            Some(miss) => {
                                let miss = crate::ParseErrorType::Resolution((**miss).clone());
                                error = crate::ParseError(
                                    Box::new(miss),
                                    error.position(),
                                    error.2.clone(),
                                );
                                proving = Some(call);
                            }
                        }
                    }
                    let proving = |error: super::Error| match &proving {
                        Some(call) => error.with_rust_application(call.clone()),
                        None => error,
                    };
                    let demand = match error.err_type() {
                        crate::ParseErrorType::Resolution(resolution) => {
                            match rust_miss(contract, resolution) {
                                Some(Miss::Demand(path, through_alias, member)) => {
                                    Some((path, through_alias, member))
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
                        return Err(proving(match demand {
                            Some((path, through_alias, member)) => error
                                .with_rust_demand(&path, through_alias, member)
                                .with_rust_misses(misses),
                            None => error.with_rust_misses(misses),
                        }));
                    }
                    let error = super::Error::compilation(sources.clone(), evidence, error);
                    return Err(proving(match demand {
                        Some((path, through_alias, member)) => error
                            .with_rust_demand(&path, through_alias, member)
                            .with_rust_misses(misses),
                        None => error.with_rust_misses(misses),
                    }));
                }
            };
        let mut compiler = contract
            .compiler_with_allocation(Brand::mint(facade), imported_snapshot, limits, allocation)
            .map_err(Error::from)
            .map_err(super::Error::import)?;
        let impls = symbolic.impls;
        for used in symbolic.uses {
            // The script's impls for the script types the instance takes,
            // which cross with them (D594).
            let crossing = crossing_impls(&used.types, &impls);
            let source = program
                .source()
                .table()
                .resolve(program.source().key(used.source))
                .expect("a use's source is in its program's source table");
            // The compiler records each use at the span it read in this text.
            let range = |span| {
                crate::resolve::lower_nominal::imported_range(source.text(), span)
                    .expect("a use's span lies in its source's text")
            };
            if let Some(identity) = used.application {
                let application = applications
                    .iter()
                    .find(|application| *application.identity() == identity)
                    .expect("a use names an application the compilation was given");
                let (start, end) = range(used.span);
                compiler
                    .require_application(application, source.identity(), start, end)
                    .map_err(Error::from)
                    .map_err(super::Error::import)?;
                continue;
            }
            let mut identity = None;
            for outcome in paths.outcomes(used.path.as_str()) {
                if !contract.accepts(outcome.identity(), &used.selector) {
                    continue;
                }
                assert!(
                    identity.replace(outcome.identity()).is_none(),
                    "one crate item at a path accepts a use's selector"
                );
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
                        Some("not supported in Rawr yet".into()),
                    )
                    .map_err(super::Error::diagnostic)?
                    .finish();
                    let mut outcomes = Vec::new();
                    let retained = paths.outcomes(used.path.as_str());
                    outcomes
                        .try_reserve_exact(retained.len())
                        .map_err(|_| super::Error::import(Error::Allocation))?;
                    outcomes.extend(retained);
                    let report = augment_external(report.clone(), outcomes, &paths)
                        .map_err(super::Error::diagnostic)?
                        .unwrap_or(report);
                    return Err(super::Error::import_diagnostic(report));
                }
            };
            let (start, end) = range(used.span);
            if used.types.is_empty() && used.functions.is_empty() && used.slots.is_empty() {
                compiler
                    .require_selected(identity, used.selector, source.identity(), start, end)
                    .map_err(|error| {
                        unsupported_use(
                            error,
                            &used.path,
                            source.identity(),
                            source.text(),
                            used.span,
                        )
                    })?;
                continue;
            }
            let mut functions = Vec::new();
            functions
                .try_reserve_exact(used.functions.len())
                .map_err(|_| super::Error::import(Error::Allocation))?;
            for function in used.functions {
                let mut selected = None;
                for outcome in paths.outcomes(function.path.as_str()) {
                    if !contract.accepts(outcome.identity(), &function.selector) {
                        continue;
                    }
                    assert!(
                        selected.replace(outcome.identity()).is_none(),
                        "one crate item at a path accepts a use's selector"
                    );
                }
                let selected = selected.ok_or_else(|| {
                    super::Error::import(Error::UnmappedImportedUse {
                        path: function.path.as_str().into(),
                        selector: function.selector.clone(),
                    })
                })?;
                let (function_start, function_end) = range(function.span);
                // A generic function, at the types its use infers.
                let item = if function.types.is_empty() {
                    rawr_import::surface::instance::FunctionItem::selected(
                        function.position,
                        selected,
                        function.selector,
                        function_start,
                        function_end,
                    )
                } else {
                    rawr_import::surface::instance::Arguments::new(
                        function.types,
                        Box::default(),
                        Box::default(),
                        None,
                        limits,
                    )
                    .and_then(|instance| {
                        rawr_import::surface::instance::FunctionItem::instantiated(
                            function.position,
                            selected,
                            function.selector,
                            instance,
                            function_start,
                            function_end,
                        )
                    })
                };
                functions.push(item.map_err(Error::from).map_err(super::Error::import)?);
            }
            let mut mappers = Vec::new();
            mappers
                .try_reserve_exact(used.mappers.len())
                .map_err(|_| super::Error::import(Error::Allocation))?;
            for mapper in used.mappers {
                let (mapper_start, mapper_end) = range(mapper.span);
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
            // A closure argument is a one-shot callable (D513.7 b), or a
            // `'static` one native code may call again, shared or mutable
            // (D551.6).
            let class = contract
                .outcome(identity)
                .and_then(|outcome| match outcome.representability() {
                    Representability::FunctionItem(plan) => plan.class(),
                    Representability::Invocation(plan) => plan.class(),
                    _ => None,
                })
                .filter(|_| used.callable);
            // A closure the native call runs only while it runs, as
            // `PaneGrid::new`'s pane closure is (D558 tier 2).
            let binders = contract
                .outcome(identity)
                .and_then(|outcome| match outcome.representability() {
                    Representability::FunctionItem(plan) => Some(plan.binders()),
                    Representability::Invocation(plan) => Some(plan.binders()),
                    _ => None,
                })
                .map(|binders| binders.to_vec().into_boxed_slice())
                .unwrap_or_default();
            // Script functions that fill callables as slots (D594) take
            // no closure class.
            let class = class.filter(|_| used.slots.is_empty());
            let slotted = !used.slots.is_empty();
            let instance = if slotted {
                rawr_import::surface::instance::Arguments::new(
                    used.types,
                    Box::default(),
                    Box::default(),
                    None,
                    limits,
                )
                .and_then(|instance| instance.with_slots(used.slots))
            } else if let Some(rawr_import::surface::callable::Class::Borrowed(_)) = class {
                rawr_import::surface::instance::Arguments::borrowed(
                    used.types,
                    Box::default(),
                    binders,
                    limits,
                )
            } else if let Some(rawr_import::surface::callable::Class::Shared(proof)) = class {
                rawr_import::surface::instance::Arguments::shared(
                    used.types,
                    Box::default(),
                    binders,
                    proof.scope(),
                    limits,
                )
            } else if let Some(rawr_import::surface::callable::Class::Mutable(_)) = class {
                rawr_import::surface::instance::Arguments::mutable(
                    used.types,
                    Box::default(),
                    binders,
                    limits,
                )
            } else if used.callable && !slotted {
                rawr_import::surface::instance::Arguments::once(
                    used.types,
                    Box::default(),
                    binders,
                    limits,
                )
            } else {
                rawr_import::surface::instance::Arguments::new(
                    used.types,
                    Box::default(),
                    Box::default(),
                    None,
                    limits,
                )
            }
            .map(|instance| instance.with_impls(crossing))
            .and_then(|instance| instance.with_function_items(functions.into_boxed_slice()))
            .and_then(|instance| instance.with_event_mappers(mappers.into_boxed_slice()))
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
                .map_err(|error| {
                    unsupported_use(
                        error,
                        &used.path,
                        source.identity(),
                        source.text(),
                        used.span,
                    )
                })?;
        }

        snapshot
            .revalidate()
            .map_err(Error::from)
            .map_err(super::Error::import)?;
        let demand = compiler
            .seal()
            .map_err(Error::from)
            .map_err(super::Error::import)?;
        Ok(CompiledImported { demand })
    }
}

/// A crate item's use the program wrote that the build of its dependencies
/// can't take at the types or arguments it gives, reported at that use in
/// Rust's terms (D664): the call and the item. Any other failure is the
/// build's own.
fn unsupported_use(
    error: rawr_import::surface::Error,
    path: &str,
    identity: &str,
    text: &str,
    span: crate::Span,
) -> super::Error {
    use rawr_import::surface::Error as Surface;
    let (message, label) = match error {
        Surface::UnconstrainedGeneric => (
            format!(
                "this use of `{path}` leaves one of its type parameters unknown, which isn't \
                 supported yet"
            ),
            "type parameter not known here",
        ),
        Surface::InstanceMismatch => (
            format!("calling `{path}` with these arguments isn't supported yet"),
            "not supported in Rawr yet",
        ),
        error => return super::Error::import(Error::from(error)),
    };
    let report = diagnostic::Source::new(identity, text).and_then(|source| {
        diagnostic::Report::error(diagnostic::code::RESOLVE_HOST_OPERATION, message)
            .primary(source, span, Some(label.into()))
            .map(diagnostic::Builder::finish)
    });
    match report {
        Ok(report) => super::Error::import_diagnostic(report),
        Err(error) => super::Error::diagnostic(error),
    }
}

/// Admit each proof-backed application's method into `registry`, by the key
/// of the source call it answers, owned by its receiver's facade. The host
/// and a generated runner admit the same applications the same way.
pub(crate) fn admit_applications(
    registry: &mut host::Registry,
    applications: &[rawr_import::surface::application::Application],
) -> Result<(), Error> {
    for application in applications {
        let signature = application.signature();
        let mut builder = host::signature::Builder::new();
        for input in signature.inputs() {
            builder.try_parameter(Types::convert_charged(input, &[])?)?;
        }
        builder.output(Types::convert_charged(signature.output(), &[])?);
        let path = format!("{}::{}", application.receiver_path(), application.name());
        let item = host::item::Definition::method(
            path,
            receiver(application.ownership()),
            builder.finish(),
        )?
        .imported(rawr_import::surface::identity::Selector::Method {
            item: application.declaration(),
            receiver: application.ownership(),
        })
        .applied(*application.identity());
        registry.admit_applied(application.call().key(), application.root(), item)?;
    }
    Ok(())
}

/// The impls in `impls` for the script types `types` name at any depth,
/// each once.
fn crossing_impls(
    types: &[representability::convert::Type],
    impls: &[(Box<str>, rawr_import::surface::instance::ScriptImpl)],
) -> Box<[rawr_import::surface::instance::ScriptImpl]> {
    fn paths<'a>(ty: &'a representability::convert::Type, found: &mut Vec<&'a str>) {
        use representability::convert::Type;
        match ty {
            Type::Script { path, arguments } => {
                found.push(path);
                arguments.iter().for_each(|argument| paths(argument, found));
            }
            Type::Nominal { arguments, .. } | Type::Tuple(arguments) => {
                arguments.iter().for_each(|argument| paths(argument, found));
            }
            Type::Option(inner)
            | Type::Range(inner)
            | Type::Borrowed { ty: inner, .. }
            | Type::Sequence { element: inner, .. }
            | Type::ProfileMessage { payload: inner, .. } => paths(inner, found),
            Type::Result(ok, err) => {
                paths(ok, found);
                paths(err, found);
            }
            Type::FunctionItem { input, output } => {
                paths(input, found);
                paths(output, found);
            }
            Type::Unit | Type::Never | Type::Builtin(_) | Type::Parameter(_) | Type::Inferred => {}
        }
    }
    if impls.is_empty() {
        return Box::default();
    }
    let mut found = Vec::new();
    for ty in types {
        paths(ty, &mut found);
    }
    impls
        .iter()
        .filter(|(owner, _)| found.contains(&owner.as_ref()))
        .map(|(_, script)| script.clone())
        .collect()
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
        return augment_external(report, outcomes, paths);
    };
    let outcomes = paths.outcomes(path.as_str());
    if outcomes.len() == 0 {
        return Ok(None);
    }
    let mut evidence = Vec::new();
    evidence
        .try_reserve_exact(outcomes.len())
        .map_err(|_| diagnostic::Error::Allocation)?;
    let mut declared = None;
    for outcome in outcomes {
        if let Some(retained) = unavailable_evidence(outcome) {
            evidence.push(retained);
            continue;
        }
        // An item whose signature names a type no facade declares is left
        // out of the facade, as a function beside a module of its name can
        // be (`mod text` and `fn text`).
        let declared = declared.get_or_insert_with(|| paths.declared());
        if !names_declared(outcome, declared)
            && let Some(origin) = outcome.origin()
        {
            evidence.push(UnavailableEvidence {
                reason: UNDECLARED.into(),
                source: origin.source(),
                declaration: origin.declaration(),
                declared: DEFINED,
            });
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
        format!("`{path}` can't be used yet: {reason}"),
    )
    .primary(source.clone(), *reference, None)?;
    let mut external = Vec::new();
    let external_capacity = evidence
        .len()
        .checked_mul(2)
        .ok_or(diagnostic::Error::Allocation)?;
    external
        .try_reserve_exact(external_capacity)
        .map_err(|_| diagnostic::Error::Allocation)?;
    for unavailable in &evidence {
        external.push((unavailable.declaration, unavailable.declared));
        if unavailable.declaration != unavailable.source {
            external.push((unavailable.source, "exported here"));
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

/// Why an item whose signature names a type no facade declares can't be
/// used.
const UNDECLARED: &str = "its signature names a type that isn't supported yet";

/// The label at an unavailable item's definition.
const DEFINED: &str = "defined here";

struct UnavailableEvidence<'a> {
    reason: Box<str>,
    /// Where the path is made public.
    source: &'a rawr_import::api::model::Span,
    /// Where the item is defined, or where it was excluded.
    declaration: &'a rawr_import::api::model::Span,
    /// The label at `declaration`.
    declared: &'static str,
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
            declared: "excluded here",
        },
        R::Rejected(reason) => UnavailableEvidence {
            reason: reason.describe().into(),
            source: reason.span(),
            declaration: reason.span(),
            declared: DEFINED,
        },
        R::Value(_) => UnavailableEvidence {
            reason: "it is a type alias, which isn't supported as a path yet".into(),
            source: outcome.origin()?.source(),
            declaration: outcome.origin()?.declaration(),
            declared: DEFINED,
        },
        R::FunctionItem(plan) if callable_generic(plan) => return None,
        R::FunctionItem(_) => UnavailableEvidence {
            reason: "it is a generic function with a const or closure parameter".into(),
            source: outcome.origin()?.source(),
            declaration: outcome.origin()?.declaration(),
            declared: DEFINED,
        },
        R::Invocation(plan)
            if !writes_declared(plan.signature(), plan.call(), plan.crossings()) =>
        {
            UnavailableEvidence {
                reason: representability::reject::Kind::CallableProtocol
                    .describe()
                    .into(),
                source: outcome.origin()?.source(),
                declaration: outcome.origin()?.declaration(),
                declared: DEFINED,
            }
        }
        R::Invocation(_) => return None,
        R::Struct(plan) if plan.declaration_only() => UnavailableEvidence {
            reason: "it is a generic struct, which can't be constructed yet".into(),
            source: outcome.origin()?.source(),
            declaration: outcome.origin()?.declaration(),
            declared: DEFINED,
        },
        R::Enum(plan) if plan.declaration_only() => UnavailableEvidence {
            reason: "it is a generic enum, which can't be constructed yet".into(),
            source: outcome.origin()?.source(),
            declaration: outcome.origin()?.declaration(),
            declared: DEFINED,
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
    /// knows which those are. The last flag marks a member of a type.
    Demand(String, bool, bool),
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
        R::NotInModule { module, item, .. } => format!("{module}::{item}"),
        _ => return None,
    };
    // A member of a type not known yet, as a second pass past a miss may
    // ask for (`_::Image`), names no crate path to demand.
    if !path.contains("::") || path.split("::").any(|segment| segment == "_") {
        return None;
    }
    let through_alias = contract.reaches_through_crate_alias(&path);
    // A member missing from a resolved owner may be one a trait the type
    // implements provides. A demanded type brings its own impl tables, and
    // demanding `Type::member` brings the member of each trait it
    // implements. Demanded once, a member still missing is a real miss.
    if matches!(error, R::UnknownMember { .. }) {
        return Some(Miss::Demand(path, through_alias, true));
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
        Some(Lookup::ThroughCrate) => Some(Miss::Demand(path, true, false)),
        _ => Some(Miss::Demand(path, through_alias, false)),
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
        R::InvalidHostOperation { item, .. } | R::ImportedApplicationRequired { item, .. } => {
            Some((item.to_string(), ImportedNamespace::Either))
        }
        // A module's path called as a function, as iced's `combo_box`: the
        // function beside it says why rawr can't call it.
        R::UnknownImport { path, .. } => Some((path.to_string(), ImportedNamespace::Either)),
        R::NotInModule { module, item, .. } => {
            Some((format!("{module}::{item}"), ImportedNamespace::Either))
        }
        R::ArgumentCount { name, .. } => Some((name.to_string(), ImportedNamespace::Value)),
        R::InvalidMethodCall { owner, method, .. }
        | R::InvalidMethodSignature { owner, method, .. } => Some((
            crate::resolve::Qualified(owner, method).to_string(),
            ImportedNamespace::Value,
        )),
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
        | R::PrivateItem { .. }
        | R::UnknownType { .. }
        | R::UnknownFunction { .. }
        | R::UnknownVariable { .. }
        | R::UnknownMacro { .. }
        | R::WrongKind { .. }
        | R::Unimplemented { .. }
        | R::IfWithoutElse { .. }
        | R::ReturnWithoutValue { .. }
        | R::MovedValue { .. }
        | R::BorrowConflict { .. }
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
        | R::Syntax { .. }
        | R::UnlabeledJumpInWhileCondition { .. }
        | R::AsyncMain { .. }
        | R::StdPath { .. }
        | R::NotYet { .. }
        | R::TypeAnnotationsNeeded { .. }
        | R::CannotCompare { .. }
        | R::ArgumentSignature { .. }
        | R::MemberKind { .. } => None,
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
    let mut outcomes = match diagnostic_path(resolution) {
        Some((path, namespace)) => diagnostic_outcomes(paths, &path, namespace)?,
        None => Vec::new(),
    };
    // A member the importer saw and couldn't bind: its own outcome says why.
    if let crate::resolve::Error::UnknownMember { owner, member, .. } = resolution {
        let retained = paths.outcomes(&format!("{owner}::{member}"));
        outcomes
            .try_reserve(retained.len())
            .map_err(|_| diagnostic::Error::Allocation)?;
        outcomes.extend(retained);
    }
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
    augment_external(report, outcomes, paths)
}

fn augment_external(
    report: diagnostic::Report,
    outcomes: Vec<rawr_import::classify::Outcome<'_>>,
    paths: &ImportedPaths<'_>,
) -> Result<Option<diagnostic::Report>, diagnostic::Error> {
    let mut spans = Vec::new();
    let span_capacity = outcomes
        .len()
        .checked_mul(2)
        .ok_or(diagnostic::Error::Allocation)?;
    spans
        .try_reserve_exact(span_capacity)
        .map_err(|_| diagnostic::Error::Allocation)?;
    let mut reasons = Vec::new();
    for outcome in outcomes {
        // Where the item is defined, and the path that makes it public
        // when that is elsewhere, as a `pub use` is: the first of a span's
        // labels is kept.
        if let Some(origin) = outcome.origin() {
            spans.push((
                origin.declaration(),
                0,
                format!("`{}` is defined here", outcome.name()),
            ));
            spans.push((
                origin.source(),
                1,
                format!("`{}` is exported here", outcome.name()),
            ));
        }
        let reason = match outcome.representability() {
            Representability::Rejected(reason) => Some(reason.describe()),
            Representability::Invocation(plan)
                if !writes_declared(plan.signature(), plan.call(), plan.crossings()) =>
            {
                Some(representability::reject::Kind::CallableProtocol.describe())
            }
            // An item whose signature names a type no facade declares is
            // left out of the facade (`names_declared`).
            _ if !names_declared(outcome, &paths.declared()) => Some(UNDECLARED),
            _ => None,
        };
        if let Some(reason) = reason {
            reasons.push(format!("`{}` can't be used yet: {reason}", outcome.name()));
        }
    }
    spans.sort_unstable();
    spans.dedup_by(|later, earlier| later.0 == earlier.0);
    reasons.sort_unstable();
    reasons.dedup();
    if spans.is_empty() && reasons.is_empty() {
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
    for reason in reasons {
        builder = builder.note(reason);
    }
    builder.reserve_external_labels(spans.len())?;
    for (span, _, label) in spans {
        builder = builder.secondary_external(external_position(span)?, Some(label.into()));
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

    /// The crate types a facade's items can name, as the facade build reads
    /// them: each struct and enum outcome's definition, in its own facade or
    /// in the one it resolves the type through.
    fn declared(&self) -> std::collections::BTreeSet<rawr_import::surface::identity::Definition> {
        self.entries
            .iter()
            .filter(|entry| entry.outcome.variant().is_none())
            .filter(|entry| {
                matches!(
                    entry.outcome.representability(),
                    Representability::Struct(_) | Representability::Enum(_)
                )
            })
            .filter_map(|entry| entry.outcome.identity().definition())
            .collect()
    }
}

fn imported_paths<'contract>(
    contract: &'contract rawr_import::Contract<rawr_import::Classified>,
    allocation: &mut rawr_import::contract::Allocation,
) -> Result<ImportedPaths<'contract>, Error> {
    let mut capacity = contract.outcomes().len();
    for outcome in contract.outcomes() {
        // Each associated item, and each member of the opaque type a
        // trait's method over its `Self` gives (`surface::opaque`).
        let associated = outcome
            .implementations()
            .flat_map(rawr_import::classify::ImplementationOutcome::associated)
            .map(|associated| {
                associated
                    .implementations()
                    .map(rawr_import::classify::ImplementationOutcome::associated)
                    .map(|members| members.len())
                    .try_fold(1_usize, usize::checked_add)
            })
            .try_fold(0_usize, |count, items| count.checked_add(items?))
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
        // A function's members are those of the opaque type it gives.
        let owner = if matches!(
            outcome.representability(),
            Representability::Function(_)
                | Representability::FunctionItem(_)
                | Representability::Invocation(_)
        ) {
            rawr_import::surface::opaque::path(&path, 0)
        } else {
            path.clone()
        };
        let definition = outcome.identity().definition();
        for (implementation, associated) in outcome.implementations().flat_map(|implementation| {
            implementation
                .associated()
                .map(move |associated| (implementation.identity(), associated))
        }) {
            // A trait's method over its `Self` is reached beside the trait
            // (`surface::member`).
            if definition.is_some_and(|definition| {
                rawr_import::surface::member::is_members(implementation, &definition)
            }) {
                let associated_path = rawr_import::surface::member::path(&path, associated.name());
                let owner = rawr_import::surface::opaque::path(&associated_path, 0);
                for member in associated
                    .implementations()
                    .flat_map(rawr_import::classify::ImplementationOutcome::associated)
                {
                    let member_path = format!("{owner}::{}", member.name());
                    allocation.retain(1, member_path.len())?;
                    entries.push(ImportedPath {
                        path: member_path.into_boxed_str(),
                        outcome: member,
                    });
                }
                allocation.retain(1, associated_path.len())?;
                entries.push(ImportedPath {
                    path: associated_path.into_boxed_str(),
                    outcome: associated,
                });
                continue;
            }
            // A method of a trait's impl for `&str` is reached beside the
            // trait (`surface::text`).
            if rawr_import::surface::text::is_member(associated.representability()) {
                let associated_path = rawr_import::surface::text::path(&path, associated.name());
                allocation.retain(1, associated_path.len())?;
                entries.push(ImportedPath {
                    path: associated_path.into_boxed_str(),
                    outcome: associated,
                });
                continue;
            }
            let path = &owner;
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
            associated_path.push_str(path);
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
    assert_eq!(
        entries.len(),
        capacity,
        "the path index holds each item and each associated item once"
    );
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
    /// Each trait the program names with its identity, whose header fixes
    /// a type its defaults leave open (D604).
    trait_identities: Vec<(Box<str>, [u8; 32])>,
    /// The re-exported enum variants, as `pub use Alignment::Center;`: each
    /// path, its enum and the variant's name.
    variants: Vec<(
        Box<str>,
        rawr_import::surface::identity::Definition,
        &'contract str,
    )>,
    /// The root's `macro_rules!` macros, as in the facade plan.
    macros: Vec<&'contract rawr_import::surface::facade::Macro>,
    /// The root's derive macros, as in the facade plan.
    derives: Vec<&'contract str>,
    /// The methods the traits the program names give Rawr's string through
    /// their impls for `&str`, each at its path (`surface::text`).
    text_members: Vec<(String, rawr_import::classify::Outcome<'contract>)>,
    /// The methods of the traits the program names that take `self` by
    /// value, each a function generic over the trait's `Self` at its path,
    /// which a program's own type that implements the trait calls
    /// (`surface::member`).
    self_members: Vec<(String, rawr_import::classify::Outcome<'contract>)>,
    /// The members of the opaque type each of `self_members` gives, each at
    /// its path beside that type, as iced's `Widget::explain` gives one
    /// whose `boxed` a program calls (`surface::opaque`).
    opaque_members: Vec<(String, rawr_import::classify::Outcome<'contract>)>,
    /// The root's re-exports of std items, as iced's `time::Duration`:
    /// each path and the std path it names (`std::time::Duration`).
    std_reexports: Vec<(Box<str>, Box<str>)>,
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
    let macros = rawr_import::surface::facade::macros(contract.raw_graph())?;
    let derives = rawr_import::surface::facade::derives(contract.raw_graph())?;
    let mut facades = Vec::<Facade<'_>>::new();
    facades
        .try_reserve_exact(facade_count)
        .map_err(|_| Error::Allocation)?;
    for (root, rules) in &macros {
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
                        variants: Vec::new(),
                        macros: Vec::new(),
                        derives: Vec::new(),
                        trait_identities: Vec::new(),
                        text_members: Vec::new(),
                        self_members: Vec::new(),
                        opaque_members: Vec::new(),
                        std_reexports: Vec::new(),
                    },
                );
                index
            }
        };
        facades[index].macros.push(rules);
    }
    for rawr_import::surface::facade::Derive { root, path } in &derives {
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
                        variants: Vec::new(),
                        macros: Vec::new(),
                        derives: Vec::new(),
                        trait_identities: Vec::new(),
                        text_members: Vec::new(),
                        self_members: Vec::new(),
                        opaque_members: Vec::new(),
                        std_reexports: Vec::new(),
                    },
                );
                index
            }
        };
        facades[index].derives.push(path);
    }
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
                            variants: Vec::new(),
                            macros: Vec::new(),
                            derives: Vec::new(),
                            trait_identities: Vec::new(),
                            text_members: Vec::new(),
                            self_members: Vec::new(),
                            opaque_members: Vec::new(),
                            std_reexports: Vec::new(),
                        },
                    );
                    index
                }
            };
            if let Some(definition) = outcome.identity().definition() {
                let identities = &mut facades[index].trait_identities;
                identities.try_reserve(1).map_err(|_| Error::Allocation)?;
                identities.push((relative.clone().into_boxed_str(), *definition.as_bytes()));
            }
            for associated in outcome
                .implementations()
                .flat_map(rawr_import::classify::ImplementationOutcome::associated)
                .filter(|associated| {
                    rawr_import::surface::text::is_member(associated.representability())
                })
            {
                let members = &mut facades[index].text_members;
                members.try_reserve(1).map_err(|_| Error::Allocation)?;
                members.push((
                    rawr_import::surface::text::path(&relative, associated.name()),
                    associated,
                ));
            }
            if let Some(definition) = outcome.identity().definition() {
                for associated in outcome
                    .implementations()
                    .filter(|implementation| {
                        rawr_import::surface::member::is_members(
                            implementation.identity(),
                            &definition,
                        )
                    })
                    .flat_map(rawr_import::classify::ImplementationOutcome::associated)
                {
                    let path = rawr_import::surface::member::path(&relative, associated.name());
                    let owner = rawr_import::surface::opaque::path(&path, 0);
                    for member in associated
                        .implementations()
                        .flat_map(rawr_import::classify::ImplementationOutcome::associated)
                    {
                        let members = &mut facades[index].opaque_members;
                        members.try_reserve(1).map_err(|_| Error::Allocation)?;
                        members.push((format!("{owner}::{}", member.name()), member));
                    }
                    let members = &mut facades[index].self_members;
                    members.try_reserve(1).map_err(|_| Error::Allocation)?;
                    members.push((path, associated));
                }
            }
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
        let definition = outcome
            .identity()
            .definition()
            .expect("a type parameter's index names a declared parameter");
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
                        variants: Vec::new(),
                        macros: Vec::new(),
                        derives: Vec::new(),
                        trait_identities: Vec::new(),
                        text_members: Vec::new(),
                        self_members: Vec::new(),
                        opaque_members: Vec::new(),
                        std_reexports: Vec::new(),
                    },
                );
                index
            }
        };
        let facade = &mut facades[facade_index];
        // A re-exported variant, as `pub use Alignment::Center;`, is its
        // enum's variant.
        if let Some(variant) = outcome.variant() {
            facade
                .variants
                .try_reserve(1)
                .map_err(|_| Error::Allocation)?;
            facade
                .variants
                .push((relative.into_boxed_str(), definition, variant));
            continue;
        }
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
    // A crate's re-export of a std item is that item, so the program names
    // it by its std path (`iced::time::Duration` is `std::time::Duration`).
    for rawr_import::surface::facade::StdReexport {
        root,
        path: relative,
        std,
    } in rawr_import::surface::facade::std_reexports(contract.raw_graph())?
    {
        allocation.retain(1, relative.len() + std.len())?;
        let index = match facades.binary_search_by(|facade| facade.root.cmp(&root)) {
            Ok(index) => index,
            Err(index) => {
                let Some(root) = contract
                    .outcomes()
                    .map(|outcome| outcome.export().root_alias())
                    .find(|alias| *alias == &*root)
                else {
                    continue;
                };
                facades.try_reserve(1).map_err(|_| Error::Allocation)?;
                facades.insert(
                    index,
                    Facade {
                        root,
                        entries: Vec::new(),
                        traits: Vec::new(),
                        variants: Vec::new(),
                        macros: Vec::new(),
                        derives: Vec::new(),
                        trait_identities: Vec::new(),
                        text_members: Vec::new(),
                        self_members: Vec::new(),
                        opaque_members: Vec::new(),
                        std_reexports: Vec::new(),
                    },
                );
                index
            }
        };
        let reexports = &mut facades[index].std_reexports;
        reexports.try_reserve(1).map_err(|_| Error::Allocation)?;
        reexports.push((relative, std));
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
    // A facade resolves a type it names and another facade declares through
    // that facade, which is built first, as in the facade plan the runner
    // builds (`facade::Resolution`). An item whose signature names a type
    // no facade declares (one the importer could not model) is unavailable
    // by itself.
    let declared = facades
        .iter()
        .map(|facade| {
            facade
                .entries
                .iter()
                .filter(|entry| {
                    matches!(
                        entry.outcome.representability(),
                        Representability::Struct(_) | Representability::Enum(_)
                    )
                })
                .map(|entry| entry.definition)
                .chain(
                    opaque_types(contract, facade)
                        .into_iter()
                        .map(|(_, definition, ..)| definition),
                )
                .collect::<std::collections::BTreeSet<_>>()
        })
        .collect::<Vec<_>>();
    regions(&facades, &declared, registry);
    let resolution = rawr_import::surface::facade::Resolution::new(
        &declared
            .iter()
            .map(|declared| {
                declared
                    .iter()
                    .map(|definition| *definition.as_bytes())
                    .collect()
            })
            .collect::<Vec<_>>(),
        &facades.iter().map(facade_named).collect::<Vec<_>>(),
    );
    let mut facades = facades.into_iter().map(Some).collect::<Vec<_>>();
    let mut built = vec![None; facades.len()];
    for &index in resolution.order() {
        let facade = facades[index]
            .take()
            .expect("each crate's bindings are built once");
        let through = resolution.through(index);
        let resolved = declared[index]
            .iter()
            .copied()
            .chain(through.iter().map(|(identity, _)| {
                rawr_import::surface::identity::Definition::authenticated(*identity)
            }))
            .collect::<std::collections::BTreeSet<_>>();
        allocation.retain_checked(
            Some(through.len()),
            through.len().checked_mul(std::mem::size_of::<(
                host::type_def::import::Identity,
                host::type_def::Export,
            )>()),
        )?;
        let foreign = through
            .iter()
            .map(|(identity, provider)| {
                let identity = host::type_def::import::Identity::new(*identity);
                let export = built[*provider]
                    .as_ref()
                    .and_then(|provider: &host::facade::Definition| {
                        provider.export_imported(&identity)
                    })
                    .expect("bindings are built after those they resolve through");
                (identity, export)
            })
            .collect();
        let (definition, families) = build(
            contract,
            facade,
            &declared[index],
            &resolved,
            foreign,
            &mut shapes,
            allocation,
        )?;
        let (items, bytes) = definition.registry_usage();
        allocation.retain_checked(items, bytes)?;
        registry.install(definition.clone())?;
        registry.defer_imported(families)?;
        built[index] = Some(definition);
    }
    Ok(())
}

/// Each crate type a facade's items name, as `names_declared` reads them,
/// other than those every facade has (`facade::Facade::named`).
fn facade_named(facade: &Facade<'_>) -> std::collections::BTreeSet<[u8; 32]> {
    fn walk(
        ty: &representability::convert::Type,
        named: &mut std::collections::BTreeSet<[u8; 32]>,
    ) {
        use representability::convert::Type;
        match ty {
            Type::Nominal {
                definition,
                arguments,
            } => {
                if !rawr_import::surface::identity::Definition::is_projected(definition.as_bytes())
                    && *definition != rawr_import::surface::identity::Definition::callable()
                {
                    named.insert(*definition.as_bytes());
                }
                for argument in arguments {
                    walk(argument, named);
                }
            }
            Type::FunctionItem { input, output } | Type::Result(input, output) => {
                walk(input, named);
                walk(output, named);
            }
            Type::Option(element)
            | Type::Borrowed { ty: element, .. }
            | Type::Range(element)
            | Type::Sequence { element, .. }
            | Type::ProfileMessage {
                payload: element, ..
            } => walk(element, named),
            Type::Tuple(members)
            | Type::Script {
                arguments: members, ..
            } => {
                for member in members {
                    walk(member, named);
                }
            }
            Type::Inferred | Type::Unit | Type::Never | Type::Builtin(_) | Type::Parameter(_) => {}
        }
    }
    fn outcome(
        outcome: rawr_import::classify::Outcome<'_>,
        named: &mut std::collections::BTreeSet<[u8; 32]>,
    ) {
        let (signature, call, crossings) = match outcome.representability() {
            Representability::Function(plan) => (plan.signature(), None, &[][..]),
            Representability::Method(plan) => (plan.signature(), None, &[][..]),
            Representability::FunctionItem(plan) => {
                (plan.signature(), plan.call(), plan.crossings())
            }
            Representability::Invocation(plan) => (plan.signature(), plan.call(), plan.crossings()),
            Representability::Constant(plan) => return walk(plan.ty(), named),
            _ => return,
        };
        for input in signature.inputs() {
            walk(input, named);
        }
        walk(signature.output(), named);
        for callable in call.into_iter().chain(crossings) {
            for input in callable.inputs() {
                walk(input, named);
            }
            walk(callable.output(), named);
        }
    }
    let mut named = std::collections::BTreeSet::new();
    for entry in &facade.entries {
        outcome(entry.outcome, &mut named);
        for associated in entry
            .outcome
            .implementations()
            .flat_map(rawr_import::classify::ImplementationOutcome::associated)
        {
            outcome(associated, &mut named);
        }
    }
    for (_, associated) in facade.text_members.iter().chain(&facade.self_members) {
        outcome(*associated, &mut named);
    }
    named
}

fn build(
    contract: &rawr_import::Contract<rawr_import::Classified>,
    mut facade: Facade<'_>,
    declared: &std::collections::BTreeSet<rawr_import::surface::identity::Definition>,
    resolved: &std::collections::BTreeSet<rawr_import::surface::identity::Definition>,
    foreign: Box<[(host::type_def::import::Identity, host::type_def::Export)]>,
    shapes: &mut Vec<(
        rawr_import::surface::identity::Definition,
        crate::Shared<host::type_def::Shape>,
    )>,
    allocation: &mut rawr_import::contract::Allocation,
) -> Result<(host::facade::Definition, Vec<host::family::Pending>), Error> {
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
    // type already has its path; so is the module a macro is in.
    let macro_modules = facade.macros.iter().filter_map(|rules| {
        rules
            .path()
            .rsplit_once("::")
            .map(|(module, _)| Box::from(module))
    });
    // So is the module beside a trait that holds the methods its impls for
    // `&str` give Rawr's string (`surface::text`).
    let text_modules = facade
        .text_members
        .iter()
        .chain(&facade.self_members)
        .filter_map(|(path, _)| {
            path.rsplit_once("::")
                .map(|(module, _)| Box::<str>::from(module))
        });
    // So is the module a derive macro is in.
    let derive_modules = facade.derives.iter().filter_map(|path| {
        path.rsplit_once("::")
            .map(|(module, _)| Box::<str>::from(module))
    });
    let modules = facade
        .traits
        .iter()
        .cloned()
        .chain(macro_modules)
        .chain(derive_modules)
        .chain(text_modules)
        .collect::<Vec<_>>();
    for path in &modules {
        let prefixes = path.match_indices("::").map(|(end, _)| &path[..end]);
        for module in prefixes.chain(std::iter::once(path.as_ref())) {
            match builder.imported_module(module, allocation) {
                Ok(_) | Err(host::Error::DuplicateFacadeEntry { .. }) => {}
                Err(error) => return Err(error.into()),
            }
        }
    }

    // An opaque `impl Trait` type a function gives is a type of its own,
    // declared beside the function, at the type arguments its caller fixes.
    for (path, definition, parameters, associated) in opaque_types(contract, &facade) {
        let shape = crate::Shared::new(host::type_def::Shape::Opaque);
        match builder.imported_opaque_type(
            &path,
            imported_identity(definition),
            &[],
            parameters,
            Box::default(),
            associated,
            shape,
            allocation,
        ) {
            Ok(_) | Err(host::Error::DuplicateFacadeEntry { .. }) => {}
            Err(error) => return Err(error.into()),
        }
    }
    for entry in &facade.entries {
        let (parameters, defaults, implemented) = match entry.outcome.representability() {
            Representability::Struct(plan) => (
                names(plan.parameters(), allocation)?,
                defaults(&types, plan.parameters(), allocation)?,
                implemented([
                    plan.copy(),
                    plan.default(),
                    plan.debug(),
                    plan.cloneable(),
                    plan.partial_eq(),
                    plan.eq(),
                    plan.hash(),
                    plan.partial_ord(),
                    plan.ord(),
                    plan.iterator(),
                ]),
            ),
            Representability::Enum(plan) => (
                names(plan.parameters(), allocation)?,
                defaults(&types, plan.parameters(), allocation)?,
                implemented([
                    plan.copy(),
                    plan.default(),
                    plan.debug(),
                    plan.cloneable(),
                    plan.partial_eq(),
                    plan.eq(),
                    plan.hash(),
                    plan.partial_ord(),
                    plan.ord(),
                    plan.iterator(),
                ]),
            ),
            // A type alias of a builtin, as iced's `markdown::Uri` is `String`.
            Representability::Value(plan) => {
                if plan.parameters().is_empty()
                    && let Some(builtin) = rawr_import::surface::facade::builtin_alias(plan.ty())
                {
                    builder.builtin_alias(entry.relative.as_ref(), builtin)?;
                } else if ty_ok(plan.ty(), declared)
                    && plan
                        .parameters()
                        .iter()
                        .filter_map(representability::signature::Parameter::default)
                        .all(|default| ty_ok(default, declared))
                {
                    // Any other type alias, as `iced::Result`, names its
                    // target with its own parameters.
                    builder.type_alias(
                        entry.relative.as_ref(),
                        names(plan.parameters(), allocation)?,
                        defaults(&types, plan.parameters(), allocation)?,
                        types.convert(plan.ty(), plan.parameters(), allocation)?,
                    )?;
                }
                continue;
            }
            Representability::Function(_)
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
                        unsupported: plan.unsupported().map(|unsupported| {
                            Box::new(host::type_def::Unsupported {
                                field: unsupported.field().into(),
                                ty: unsupported.ty().into(),
                            })
                        }),
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
                            variants.push(
                                host::type_def::Variant::new(
                                    variant.name().into(),
                                    fields.into_boxed_slice(),
                                    variant.constructible(),
                                    variant.selector().try_clone()?,
                                )
                                .with_unsupported(
                                    variant.unsupported().map(|unsupported| {
                                        host::type_def::Unsupported {
                                            field: unsupported.field().into(),
                                            ty: unsupported.ty().into(),
                                        }
                                    }),
                                ),
                            );
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
                    | Representability::Rejected(_) => {
                        unreachable!("a type's entry is a struct or an enum")
                    }
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
            &implemented,
            parameters,
            defaults,
            shape,
            allocation,
        )?;
    }

    for entry in &facade.entries {
        // The values form stands alone where no closure fills the callable
        // form, as iced's `selector::find(S)` takes a widget `Id` through
        // `Selector`, which iced also implements for `FnMut` closures.
        if names_declared(entry.outcome, resolved) {
            let item = imported_item(&types, entry.relative.as_ref(), entry.outcome, allocation)?;
            if let Some(item) = item {
                builder.imported_item(item)?;
            }
            if let Some(values) =
                imported_values(&types, entry.relative.as_ref(), entry.outcome, allocation)?
            {
                builder.imported_item(values)?;
            }
        }
        // A function's members are those of the opaque type it gives.
        let owner = if matches!(
            entry.outcome.representability(),
            Representability::Function(_)
                | Representability::FunctionItem(_)
                | Representability::Invocation(_)
        ) {
            rawr_import::surface::opaque::path(&entry.relative, 0)
        } else {
            entry.relative.to_string()
        };
        for associated in entry
            .outcome
            .implementations()
            .flat_map(rawr_import::classify::ImplementationOutcome::associated)
        {
            let path_bytes = owner
                .len()
                .checked_add(2)
                .and_then(|bytes| bytes.checked_add(associated.name().len()))
                .ok_or(Error::Allocation)?;
            allocation.retain(1, path_bytes)?;
            let mut path = String::new();
            path.try_reserve_exact(path_bytes)
                .map_err(|_| Error::Allocation)?;
            path.push_str(&owner);
            path.push_str("::");
            path.push_str(associated.name());
            // An alias that passes its parameters on to a type has that
            // type's associated functions, as `widget::Text::new`; its
            // methods are reached through the type's values.
            let alias = matches!(entry.outcome.representability(), Representability::Value(_));
            if let Some(item) = imported_item(&types, &path, associated, allocation)?
                .filter(|_| names_declared(associated, resolved))
                .filter(|item| !alias || item.receiver().is_none())
            {
                builder.imported_item(item)?;
                if let Some(values) = imported_values(&types, &path, associated, allocation)?
                    .filter(|item| !alias || item.receiver().is_none())
                {
                    builder.imported_item(values)?;
                }
            }
        }
    }
    // A member of the opaque type a trait's method over its `Self` gives,
    // as a function's member is (`surface::opaque`).
    for (path, member) in &facade.opaque_members {
        if let Some(item) = imported_item(&types, path, *member, allocation)?
            .filter(|_| names_declared(*member, resolved))
        {
            builder.imported_item(item)?;
            if let Some(values) = imported_values(&types, path, *member, allocation)? {
                builder.imported_item(values)?;
            }
        }
    }
    // A trait's method its impl for `&str` gives Rawr's string: a function
    // whose first input is the string (`surface::text`).
    for (path, associated) in &facade.text_members {
        let Representability::Invocation(plan) = associated.representability() else {
            continue;
        };
        // Its signature is written with no closure, so a callable
        // parameter would be named undeclared.
        if plan.receiver()
            != Some(rawr_import::classify::representability::ownership::Receiver::Owned)
            || !writes_declared(plan.signature(), None, &[])
            || !names_declared(*associated, resolved)
        {
            continue;
        }
        let selector_bytes = associated.identity().selector().retained_bytes();
        let bytes = selector_bytes.and_then(|selector| {
            std::mem::size_of::<host::item::Definition>()
                .checked_add(unqualified_path_bytes(path)?)?
                .checked_add(selector)
        });
        allocation.retain_checked(Some(1), bytes)?;
        // The string at the impl's lifetime: an impl for `&'static str`
        // alone borrows its receiver for `'static`, as D694 lends a
        // `&'static str` input, and its inputs come after the receiver.
        let instance = plan.signature().instance().cloned().unwrap_or(
            representability::convert::Type::Builtin(representability::convert::Builtin::Str),
        );
        // A `&str` receiver, as rustc's `<&str as Trait>::method(text)`
        // takes it, not a `String` (#706).
        let receiver = types.convert_input(&instance, &[], allocation)?;
        let signature = signature_with(&types, plan.signature(), Some(receiver), allocation)?;
        let lent = (lent_static(plan.signature()) << 1)
            | u32::from(
                instance
                    == representability::convert::Type::Builtin(
                        representability::convert::Builtin::StaticStr,
                    ),
            );
        builder.item(
            host::item::Definition::function(path, signature)?
                .imported(associated.identity().selector().try_clone()?)
                .lending_static(lent),
        )?;
    }
    // A trait's method over its `Self`, which a program's own type that
    // implements the trait calls: a function whose first input is the
    // receiver (`surface::member`).
    for (path, associated) in &facade.self_members {
        let Representability::Invocation(plan) = associated.representability() else {
            continue;
        };
        if !writes_declared(plan.signature(), plan.call(), plan.crossings())
            || !names_declared(*associated, resolved)
        {
            continue;
        }
        let selector_bytes = associated.identity().selector().retained_bytes();
        let bytes = selector_bytes.and_then(|selector| {
            std::mem::size_of::<host::item::Definition>()
                .checked_add(unqualified_path_bytes(path)?)?
                .checked_add(selector)
        });
        allocation.retain_checked(Some(1), bytes)?;
        // A callable `Self`, as a function is of iced's `Function::with`,
        // is the closure's type.
        let signature = callable_signature(
            &types,
            plan.signature(),
            plan.call(),
            plan.crossings(),
            allocation,
        )?;
        builder.item(
            host::item::Definition::function(path, signature)?
                .imported(associated.identity().selector().try_clone()?),
        )?;
    }
    // The headers of the traits the program names and of the crate's trait
    // impls, which fix a type a trait's defaults leave open (D604). A header
    // naming a type the facade can't write is left out.
    let declared_bytes = declared
        .iter()
        .map(|definition| *definition.as_bytes())
        .collect::<std::collections::BTreeSet<_>>();
    // Each opaque type's parameter count, as `opaque_types` declares it.
    let opaque = opaque_types(contract, &facade)
        .into_iter()
        .map(|(_, definition, parameters, _)| (*definition.as_bytes(), parameters.len()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let (trait_headers, impl_headers) = rawr_import::surface::headers::headers(
        contract.raw_graph(),
        &declared_bytes,
        &opaque,
        &facade.trait_identities,
    );
    for header in trait_headers {
        let defaults = header
            .defaults
            .iter()
            .map(|default| {
                default
                    .as_ref()
                    .map(|default| types.convert(default, &header.parameters, allocation))
                    .transpose()
            })
            .collect::<Result<Box<[_]>, _>>();
        let Ok(defaults) = defaults else {
            continue;
        };
        builder.trait_header(
            &header.path,
            header.identity,
            header
                .parameters
                .iter()
                .map(|parameter| ImmutableString::from(parameter.name()))
                .collect(),
            defaults,
        )?;
    }
    for header in impl_headers {
        let mut convert = |ty: &representability::convert::Type| {
            types.convert(ty, &header.parameters, allocation).ok()
        };
        let Some(arguments) = header
            .arguments
            .iter()
            .map(&mut convert)
            .collect::<Option<Box<[_]>>>()
        else {
            continue;
        };
        let Some(self_type) = convert(&header.self_type) else {
            continue;
        };
        let bounds = header
            .bounds
            .iter()
            .filter_map(|bound| {
                Some(host::facade::ImplBound {
                    ty: convert(&bound.ty)?,
                    trait_identity: bound.trait_identity,
                    arguments: bound
                        .arguments
                        .iter()
                        .map(&mut convert)
                        .collect::<Option<Box<[_]>>>()?,
                })
            })
            .collect();
        builder.impl_header(host::facade::ImplHeader {
            parameters: header
                .parameters
                .iter()
                .map(|parameter| ImmutableString::from(parameter.name()))
                .collect(),
            trait_identity: header.trait_identity,
            arguments,
            self_type,
            bounds,
        })?;
    }
    // A re-exported variant names its enum's variant, through a path the
    // facade declares the enum at. One of an enum it does not declare is
    // unavailable.
    for (path, definition, variant) in &facade.variants {
        let Some(owner) = facade.entries.iter().find(|entry| {
            entry.definition == *definition
                && matches!(entry.outcome.representability(), Representability::Enum(_))
        }) else {
            continue;
        };
        let target = format!("{}::{variant}", owner.relative);
        allocation.retain(1, target.len())?;
        builder.variant_alias(path.as_ref(), &target)?;
    }
    for (path, std) in &facade.std_reexports {
        match builder.std_alias(path.as_ref(), std.as_ref()) {
            // A path the crate also declares something at keeps that.
            Ok(_) | Err(host::Error::DuplicateFacadeEntry { .. }) => {}
            Err(error) => return Err(error.into()),
        }
    }
    // A dependency's `macro_rules!` macros, which Rawr expands: `$crate`
    // reads as the defining crate's root through this root.
    for rules in &facade.macros {
        let item = host::item::Definition::macro_rules(
            rules.path(),
            rules.source(),
            format!("{}::{}", facade.root, rules.krate()),
        )?;
        match builder.item(item) {
            Ok(_) | Err(host::Error::DuplicateFacadeEntry { .. }) => {}
            Err(error) => return Err(error.into()),
        }
    }
    // A dependency's derive macros, which a program's `#[derive(..)]` names.
    for path in &facade.derives {
        builder.derive(path)?;
    }
    builder
        .finish_imported(foreign, allocation)
        .map_err(Error::from)
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
        | Representability::Constant(_) => true,
        // A callable parameter whose closure Rawr can't make, as cosmic
        // text's `with_buffer_mut<F: FnOnce(&mut Borrowed<..>) -> T, T>`
        // and tokio's `MappedMutexGuard::map<U, F: FnOnce(&mut T) -> &mut
        // U>` take, leaves the item uncallable: no binding, and the crate's
        // other items stay importable.
        Representability::Invocation(plan) => {
            writes_declared(plan.signature(), plan.call(), plan.crossings())
        }
        Representability::FunctionItem(plan) => callable_generic(plan),
        _ => false,
    };
    if !retains_item {
        return Ok(None);
    }
    let selector_bytes = outcome.identity().selector().retained_bytes();
    let bytes = selector_bytes.and_then(|selector| {
        std::mem::size_of::<host::item::Definition>()
            .checked_add(unqualified_path_bytes(path)?)?
            .checked_add(selector)
    });
    allocation.retain_checked(Some(1), bytes)?;
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
                _ => unreachable!("an item's plan is of its selector's kind"),
            }
            .imported(outcome.identity().selector().try_clone()?)
            .lending_static(lent_static(plan.signature()))
            .lending(lends(plan.signature())),
        ),
        Representability::Method(plan) => Some(
            host::item::Definition::method(
                path,
                receiver(plan.receiver()),
                signature(types, plan.signature(), allocation)?,
            )?
            .imported(outcome.identity().selector().try_clone()?)
            .guarded(guard(plan.signature()))
            .opening(plan.signature().scope().is_some())
            .lending_static(lent_static(plan.signature()))
            .lending(lends(plan.signature())),
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
                _ => unreachable!("an item's plan is of its selector's kind"),
            }
            .imported(outcome.identity().selector().try_clone()?),
        ),
        Representability::Invocation(plan) => Some(
            match outcome.identity().selector() {
                // A closure the item takes, as a generic type's `style(impl
                // Fn(&Theme, Status) -> Style + 'a)`, is the closure's type.
                Selector::AssociatedFunction { .. } => host::item::Definition::associated_function(
                    path,
                    callable_signature(
                        types,
                        plan.signature(),
                        plan.call(),
                        plan.crossings(),
                        allocation,
                    )?,
                )?,
                Selector::Method { .. } => host::item::Definition::method(
                    path,
                    receiver(plan.receiver().expect("a method's plan has its receiver")),
                    callable_signature(
                        types,
                        plan.signature(),
                        plan.call(),
                        plan.crossings(),
                        allocation,
                    )?,
                )?,
                _ => unreachable!("an item's plan is of its selector's kind"),
            }
            .imported(outcome.identity().selector().try_clone()?)
            .guarded(guard(plan.signature()))
            .opening(plan.signature().scope().is_some())
            .lending_static(lent_static(plan.signature()))
            .lending(lends(plan.signature())),
        ),
        // A generic function Rawr calls at the types it infers or a
        // turbofish names. The bridge calls it at those types, where rustc
        // checks its bounds.
        Representability::FunctionItem(plan) if callable_generic(plan) => Some(
            match outcome.identity().selector() {
                Selector::AssociatedFunction { .. } => host::item::Definition::associated_function(
                    path,
                    callable_signature(
                        types,
                        plan.signature(),
                        plan.call(),
                        plan.crossings(),
                        allocation,
                    )?,
                )?,
                Selector::Function { .. } => host::item::Definition::function(
                    path,
                    callable_signature(
                        types,
                        plan.signature(),
                        plan.call(),
                        plan.crossings(),
                        allocation,
                    )?,
                )?,
                Selector::Method { .. } => host::item::Definition::method(
                    path,
                    receiver(plan.receiver().expect("a method's plan has its receiver")),
                    callable_signature(
                        types,
                        plan.signature(),
                        plan.call(),
                        plan.crossings(),
                        allocation,
                    )?,
                )?,
                _ => unreachable!("an item's plan is of its selector's kind"),
            }
            .imported(outcome.identity().selector().try_clone()?)
            .guarded(guard(plan.signature()))
            .opening(plan.signature().scope().is_some())
            .lending_static(lent_static(plan.signature()))
            .lending(lends(plan.signature())),
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

/// The inputs of `signature`, by bit, whose borrows the output keeps for
/// as long as it lives (`Signature::lends`). An opaque output also keeps
/// those of each input whose type is a type parameter it captures, as
/// `wrap(shown: impl Show) -> impl Show` keeps what `shown` borrows (#856).
fn lends(signature: &representability::signature::Signature) -> u32 {
    use representability::convert::Type;
    fn names(ty: &Type, parameter: u32) -> bool {
        match ty {
            Type::Parameter(index) => *index == parameter,
            Type::Nominal { arguments, .. } | Type::Tuple(arguments) => {
                arguments.iter().any(|ty| names(ty, parameter))
            }
            Type::Option(inner)
            | Type::Range(inner)
            | Type::Borrowed { ty: inner, .. }
            | Type::Sequence { element: inner, .. } => names(inner, parameter),
            Type::Result(ok, error) => names(ok, parameter) || names(error, parameter),
            _ => false,
        }
    }
    let output = signature.output();
    let opaque = matches!(output, Type::Nominal { definition, .. }
        if signature
            .opaques()
            .iter()
            .any(|opaque| opaque.definition() == definition.as_bytes()));
    let captured = signature
        .inputs()
        .iter()
        .enumerate()
        .filter(|(_, input)| {
            opaque && matches!(input, Type::Parameter(parameter) if names(output, *parameter))
        })
        .filter_map(|(index, _)| u32::try_from(index).ok());
    signature
        .lends()
        .iter()
        .copied()
        .chain(captured)
        .filter(|index| *index < 32)
        .fold(0, |bits, index| bits | 1 << index)
}

/// The inputs of `signature`, by bit, that only a `&'static str` fills:
/// one written `&'static str`, as iced's `Font::with_name` takes, or a type
/// parameter a `&str` fills only at `'static`, as iced's `title(impl
/// Title<State>)` is, which iced implements for `&'static str`.
fn lent_static(signature: &representability::signature::Signature) -> u32 {
    use representability::convert::{Builtin, Type};
    signature
        .inputs()
        .iter()
        .enumerate()
        .take(32)
        .filter(|(_, input)| match input {
            Type::Builtin(Builtin::StaticStr) => true,
            Type::Parameter(index) => signature
                .parameters()
                .iter()
                .any(|parameter| parameter.index() == *index && parameter.static_str()),
            _ => false,
        })
        .fold(0, |bits, (index, _)| bits | 1 << index)
}

/// The form of `outcome`'s item that a value fills where the item takes a
/// callable through a crate's trait over `Fn` the crate also implements for
/// a type of its own, as iced's `.theme(Theme::Dark)` fills `impl
/// ThemeFn<State, Theme>` (`callable::Item::values`): another form of the
/// item, beside the one a script function fills.
fn imported_values(
    types: &Types,
    path: &str,
    outcome: rawr_import::classify::Outcome<'_>,
    allocation: &mut rawr_import::contract::Allocation,
) -> Result<Option<host::item::Definition>, Error> {
    use rawr_import::surface::identity::Selector;

    let (values, retains) = match outcome.representability() {
        Representability::FunctionItem(plan) => match plan.values() {
            Some(values) => (values, callable_generic(values)),
            None => return Ok(None),
        },
        Representability::Invocation(plan) => match plan.item().values() {
            Some(values) => (
                values,
                writes_declared(values.signature(), values.call(), values.crossings()),
            ),
            None => return Ok(None),
        },
        _ => return Ok(None),
    };
    if !retains {
        return Ok(None);
    }
    let selector = outcome.identity().selector();
    let bytes = selector.retained_bytes().and_then(|selector| {
        std::mem::size_of::<host::item::Definition>()
            .checked_add(unqualified_path_bytes(path)?)?
            .checked_add(selector)
    });
    allocation.retain_checked(Some(1), bytes)?;
    let signature = callable_signature(
        types,
        values.signature(),
        values.call(),
        values.crossings(),
        allocation,
    )?;
    let definition = match selector {
        Selector::AssociatedFunction { .. } => {
            host::item::Definition::associated_function(path, signature)?
        }
        Selector::Function { .. } => host::item::Definition::function(path, signature)?,
        Selector::Method { .. } => host::item::Definition::method(
            path,
            receiver(values.receiver().expect("a method's plan has its receiver")),
            signature,
        )?,
        _ => unreachable!("an item's plan is of its selector's kind"),
    };
    Ok(Some(
        definition
            .imported(selector.try_clone()?)
            .guarded(guard(values.signature()))
            .opening(values.signature().scope().is_some())
            .lending_static(lent_static(values.signature()))
            .lending(lends(values.signature())),
    ))
}

/// Whether every imported type the outcome's signature names is declared.
fn names_declared(
    outcome: rawr_import::classify::Outcome<'_>,
    declared: &std::collections::BTreeSet<rawr_import::surface::identity::Definition>,
) -> bool {
    let (signature, call, crossings) = match outcome.representability() {
        Representability::Function(plan) => (plan.signature(), None, &[][..]),
        Representability::Method(plan) => (plan.signature(), None, &[][..]),
        Representability::FunctionItem(plan) => (plan.signature(), plan.call(), plan.crossings()),
        Representability::Invocation(plan) => (plan.signature(), plan.call(), plan.crossings()),
        Representability::Constant(plan) => return ty_ok(plan.ty(), declared),
        _ => return true,
    };
    // What a callable it takes is given and gives, as `Cache::draw`'s
    // `&mut Frame<Renderer>`, is named in the bridge too.
    signature
        .inputs()
        .iter()
        .all(|input| ty_ok(input, declared))
        && ty_ok(signature.output(), declared)
        && call.into_iter().chain(crossings).all(|callable| {
            callable.inputs().iter().all(|input| ty_ok(input, declared))
                && ty_ok(callable.output(), declared)
        })
}

/// An opaque type a facade declares: its path, its definition, the names
/// of its parameters, and the associated types it binds.
type OpaqueType = (
    String,
    rawr_import::surface::identity::Definition,
    Box<[ImmutableString]>,
    Box<[host::type_def::Associated]>,
);

/// Records in `registry` the crate types that have lifetime parameters and
/// the opaque types the runner names by the type rustc gives them: those
/// whose every crate type has a path in a facade (`declared`), as iced's
/// `Widget::explain` gives `iced_widget::Explain<W>`. A value of another
/// opaque type that borrows can't live in a region (`resolve::
/// lower_nominal`'s `unheld_opaque`).
fn regions(
    facades: &[Facade<'_>],
    declared: &[std::collections::BTreeSet<rawr_import::surface::identity::Definition>],
    registry: &mut host::Registry,
) {
    fn named(
        ty: &representability::convert::Type,
        declared: &[std::collections::BTreeSet<rawr_import::surface::identity::Definition>],
    ) -> bool {
        use representability::convert::Type;
        match ty {
            Type::Nominal {
                definition,
                arguments,
            } => {
                (declared.iter().any(|set| set.contains(definition))
                    || rawr_import::surface::identity::Definition::std_path(definition.as_bytes())
                        .is_some())
                    && arguments.iter().all(|argument| named(argument, declared))
            }
            Type::Script { arguments, .. } | Type::Tuple(arguments) => {
                arguments.iter().all(|argument| named(argument, declared))
            }
            Type::Option(inner)
            | Type::Range(inner)
            | Type::Borrowed { ty: inner, .. }
            | Type::Sequence { element: inner, .. }
            | Type::ProfileMessage { payload: inner, .. } => named(inner, declared),
            Type::Result(ok, err) => named(ok, declared) && named(err, declared),
            Type::FunctionItem { input, output } => {
                named(input, declared) && named(output, declared)
            }
            Type::Unit | Type::Never | Type::Builtin(_) | Type::Parameter(_) | Type::Inferred => {
                true
            }
        }
    }
    let entries = || facades.iter().flat_map(|facade| facade.entries.iter());
    let scoped = entries()
        .filter(|entry| match entry.outcome.representability() {
            Representability::Struct(plan) => plan.lifetimes() > 0,
            Representability::Enum(plan) => plan.lifetimes() > 0,
            _ => false,
        })
        .map(|entry| *entry.definition.as_bytes())
        .collect::<Vec<_>>();
    // Each function's and method's signature, as `opaque_types` reads them.
    let outcomes = facades.iter().flat_map(|facade| {
        facade
            .entries
            .iter()
            .map(|entry| entry.outcome)
            .chain(facade.self_members.iter().map(|(_, outcome)| *outcome))
            .chain(facade.opaque_members.iter().map(|(_, outcome)| *outcome))
            .chain(facade.entries.iter().flat_map(|entry| {
                entry
                    .outcome
                    .implementations()
                    .flat_map(rawr_import::classify::ImplementationOutcome::associated)
            }))
    });
    let named_opaques = outcomes
        .filter_map(|outcome| match outcome.representability() {
            Representability::Function(plan) => Some(plan.signature()),
            Representability::FunctionItem(plan) => Some(plan.signature()),
            Representability::Invocation(plan) => Some(plan.signature()),
            Representability::Method(plan) => Some(plan.signature()),
            _ => None,
        })
        .flat_map(|signature| signature.opaques().iter())
        .filter(|opaque| {
            opaque
                .hidden()
                .is_some_and(|hidden| named(hidden, declared))
        })
        .map(|opaque| *opaque.definition())
        .collect::<Vec<_>>();
    registry.regions(scoped, named_opaques);
}

/// The opaque `impl Trait` types the facade's functions and methods give:
/// each one's path beside each function that gives it, its definition,
/// and its type parameters, named as the function's parameters its
/// arguments are, so its members' signatures name them.
fn opaque_types(
    contract: &rawr_import::Contract<rawr_import::Classified>,
    facade: &Facade<'_>,
) -> Vec<OpaqueType> {
    fn walk<'t>(
        ty: &'t representability::convert::Type,
        opaque: &std::collections::BTreeSet<[u8; 32]>,
        found: &mut Vec<(
            rawr_import::surface::identity::Definition,
            &'t [representability::convert::Type],
        )>,
    ) {
        use representability::convert::Type;
        match ty {
            Type::Nominal {
                definition,
                arguments,
            } => {
                if opaque.contains(definition.as_bytes()) {
                    found.push((*definition, arguments));
                }
                for argument in arguments {
                    walk(argument, opaque, found);
                }
            }
            Type::Option(inner)
            | Type::Borrowed { ty: inner, .. }
            | Type::Range(inner)
            | Type::Sequence { element: inner, .. } => walk(inner, opaque, found),
            Type::Result(ok, err) => {
                walk(ok, opaque, found);
                walk(err, opaque, found);
            }
            Type::Tuple(members) => {
                for member in members {
                    walk(member, opaque, found);
                }
            }
            _ => {}
        }
    }
    let opaque = rawr_import::surface::opaque::identities(contract.raw_graph());
    if opaque.is_empty() {
        return Vec::new();
    }
    let mut types: Vec<OpaqueType> = Vec::new();
    // `member` marks a member's output, which only names a type: one already
    // declared beside a function isn't declared again.
    let mut visit = |path: &str, outcome: rawr_import::classify::Outcome<'_>, member: bool| {
        let signature = match outcome.representability() {
            Representability::Function(plan) => plan.signature(),
            Representability::FunctionItem(plan) => plan.signature(),
            Representability::Invocation(plan) => plan.signature(),
            Representability::Method(plan) => plan.signature(),
            _ => return,
        };
        let mut found = Vec::new();
        walk(signature.output(), &opaque, &mut found);
        // Declared beside each path to the function, which its members
        // are reached through.
        for (index, (definition, arguments)) in found.into_iter().enumerate() {
            let path = rawr_import::surface::opaque::path(path, index);
            if types
                .iter()
                .any(|(known, declared, ..)| *known == path || (member && *declared == definition))
            {
                continue;
            }
            let names = arguments
                .iter()
                .enumerate()
                .map(|(position, argument)| match argument {
                    representability::convert::Type::Parameter(index) => signature
                        .parameters()
                        .iter()
                        .find(|parameter| parameter.index() == *index)
                        .map_or_else(
                            || format!("T{position}").into_boxed_str(),
                            |parameter| parameter.name().into(),
                        ),
                    _ => format!("T{position}").into_boxed_str(),
                })
                .collect();
            // The associated types it binds, each one of its arguments.
            let bound = signature
                .opaques()
                .iter()
                .find(|opaque| opaque.definition() == definition.as_bytes())
                .map_or(&[][..], |opaque| opaque.associated());
            let names = rawr_import::surface::opaque::parameters(names, bound)
                .iter()
                .map(|name| ImmutableString::from(&**name))
                .collect();
            let associated = bound
                .iter()
                .map(|(associated, position)| {
                    (
                        associated.trait_identity(),
                        ImmutableString::from(associated.name()),
                        *position as usize,
                    )
                })
                .collect();
            types.push((path, definition, names, associated));
        }
    };
    for entry in &facade.entries {
        visit(&entry.relative, entry.outcome, false);
    }
    // A trait's method over its `Self` is a function of its own
    // (`surface::member`), as the runner's facade declares it.
    for (path, associated) in &facade.self_members {
        visit(path, *associated, false);
    }
    for (path, member) in &facade.opaque_members {
        visit(path, *member, true);
    }
    // A member's opaque output is declared in its owner's module, as
    // `image::io::__rawr_opaque_Reader__into_decoder` for `Reader`'s
    // `into_decoder`: a type or a function is no module to declare it in.
    for entry in &facade.entries {
        for associated in entry
            .outcome
            .implementations()
            .flat_map(rawr_import::classify::ImplementationOutcome::associated)
        {
            visit(
                &format!("{}__{}", entry.relative, associated.name()),
                associated,
                true,
            );
        }
    }
    types
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
                || rawr_import::surface::identity::Definition::is_projected(definition.as_bytes())
                || *definition == rawr_import::surface::identity::Definition::callable())
                && arguments.iter().all(|ty| ty_ok(ty, declared))
        }
        Type::FunctionItem { input, output } | Type::Result(input, output) => {
            ty_ok(input, declared) && ty_ok(output, declared)
        }
        Type::Option(element)
        | Type::Borrowed { ty: element, .. }
        | Type::Range(element)
        | Type::Sequence { element, .. } => ty_ok(element, declared),
        Type::Tuple(members)
        | Type::Script {
            arguments: members, ..
        } => members.iter().all(|ty| ty_ok(ty, declared)),
        Type::ProfileMessage { payload, .. } => ty_ok(payload, declared),
        Type::Inferred => true,
        Type::Unit | Type::Never | Type::Builtin(_) | Type::Parameter(_) => true,
    }
}

/// A generic item Rawr calls directly: every parameter a type, none a
/// callable (those are function-item operands).
fn callable_generic(plan: &representability::callable::Item) -> bool {
    let kinds = match plan.class() {
        // A script function fills each callable parameter as a slot,
        // as `iced::run(update, view)` takes them (D594).
        None if !plan.crossings().is_empty() => {
            plan.signature().parameters().iter().all(|parameter| {
                matches!(
                    parameter.kind(),
                    representability::signature::Kind::Type
                        | representability::signature::Kind::Future
                        | representability::signature::Kind::Callable
                )
            })
        }
        // `F: Future` takes a script future (D527).
        None => plan.signature().parameters().iter().all(|parameter| {
            matches!(
                parameter.kind(),
                representability::signature::Kind::Type | representability::signature::Kind::Future
            )
        }),
        // `F: FnOnce(..) -> R`, as `spawn_blocking` takes: a script closure
        // runs there (D513.7 b). `F: Fn(..) -> R + 'static`, as Freya's
        // `AppComponent` holds, may call it again, and so may
        // `F: FnMut(..) -> R + 'static`, as Freya's `EventHandler` holds.
        // A closure that takes a `&mut` borrow, as iced's `Path::new(impl
        // FnOnce(&mut Builder))` gives one, is described by its crossing,
        // as it is for a generic type's `Cache::draw`. An `Fn` or `FnMut`
        // with no `'static` bound, as image's `ImageBuffer::from_fn(w, h,
        // |x, y| ..)` takes, is borrowed for the call alone.
        Some(
            rawr_import::surface::callable::Class::Once(_)
            | rawr_import::surface::callable::Class::Shared(_)
            | rawr_import::surface::callable::Class::Mutable(_)
            | rawr_import::surface::callable::Class::Borrowed(_),
        ) => plan.call().is_some() || !plan.crossings().is_empty(),
    };
    kinds && writes_declared(plan.signature(), plan.call(), plan.crossings())
}

/// Whether the binding of `signature`, with a script closure (`call`) or a
/// script function's slots (`crossings`) for its callables, names only the
/// type parameters it declares. A signature declares only its type
/// parameters and writes each filled callable as the closure's type. The
/// classifier keeps items that break this when it can't name what a
/// callable takes or gives, as tokio's `MappedMutexGuard::map` and cosmic
/// text's `with_buffer_mut` are, or when a closure gives a future parameter,
/// as warp's `Ws::on_upgrade(F) where F: FnOnce(WebSocket) -> U, U: Future`
/// does. Such an item has no binding (T313).
fn writes_declared(
    signature: &representability::signature::Signature,
    call: Option<&representability::callable::Call>,
    crossings: &[representability::callable::Call],
) -> bool {
    use representability::convert::Type;
    use representability::signature::Kind;

    fn declared(ty: &Type, parameters: &[representability::signature::Parameter]) -> bool {
        match ty {
            Type::Parameter(index) => parameters
                .iter()
                .find(|parameter| parameter.index() == *index)
                .is_none_or(|parameter| parameter.kind() == Kind::Type),
            Type::Nominal { arguments, .. }
            | Type::Tuple(arguments)
            | Type::Script { arguments, .. } => arguments.iter().all(|ty| declared(ty, parameters)),
            Type::Option(element)
            | Type::Borrowed { ty: element, .. }
            | Type::Range(element)
            | Type::Sequence { element, .. }
            | Type::ProfileMessage {
                payload: element, ..
            } => declared(element, parameters),
            Type::Result(first, second)
            | Type::FunctionItem {
                input: first,
                output: second,
            } => declared(first, parameters) && declared(second, parameters),
            Type::Unit | Type::Never | Type::Builtin(_) | Type::Inferred => true,
        }
    }

    let parameters = signature.parameters();
    let calls = || call.into_iter().chain(crossings);
    let filled = parameters
        .iter()
        .filter(|parameter| parameter.kind() == Kind::Callable)
        .all(|parameter| calls().any(|call| call.parameter() == parameter.index()));
    // An input a closure fills, or an `Option` of the one Rawr makes, is
    // written as the closure's type (`callable_signature`).
    let closure = |input: &Type| {
        calls().any(|filled| *input == Type::Parameter(filled.parameter()))
            || call.is_some_and(|call| {
                matches!(input, Type::Option(inner)
                    if **inner == Type::Parameter(call.parameter()))
            })
    };
    filled
        && signature
            .inputs()
            .iter()
            .filter(|input| !closure(input))
            .chain([signature.output()])
            .all(|ty| declared(ty, parameters))
        && calls().all(|call| {
            call.inputs()
                .iter()
                .chain([call.output()])
                .all(|ty| declared(ty, parameters))
        })
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

    /// An item's input. A `&str` or `&'static str` is a shared reference to
    /// the string, as rustc sees it, so it stays apart from a `String`
    /// input: iced's `Id::from` has both, and the argument picks one.
    fn convert_input(
        &self,
        ty: &representability::convert::Type,
        parameters: &[representability::signature::Parameter],
        allocation: &mut rawr_import::contract::Allocation,
    ) -> Result<host::type_ref::Type, Error> {
        let (items, bytes) = type_usage(ty, parameters)?;
        allocation.retain(items, bytes)?;
        Self::input_charged(ty, parameters)
    }

    /// `convert_input` of a charged type. A `&str` that an input holds, as
    /// a future's `Option<&str>` or `(&str, &Catalog)` does, is a borrow
    /// there too (#677).
    fn input_charged(
        ty: &representability::convert::Type,
        parameters: &[representability::signature::Parameter],
    ) -> Result<host::type_ref::Type, Error> {
        use representability::convert::{Builtin, Type};
        Ok(match ty {
            Type::Builtin(Builtin::Str | Builtin::StaticStr) => {
                host::type_ref::Type::reference(false, Self::convert_charged(ty, parameters)?)
            }
            Type::Option(element) => {
                host::type_ref::Type::option(Self::input_charged(element, parameters)?)
            }
            Type::Tuple(members) => host::type_ref::Type::tuple(
                members
                    .iter()
                    .map(|member| Self::input_charged(member, parameters))
                    .collect::<Result<Vec<_>, _>>()?,
            )?,
            _ => Self::convert_charged(ty, parameters)?,
        })
    }

    fn convert_charged(
        ty: &representability::convert::Type,
        parameters: &[representability::signature::Parameter],
    ) -> Result<host::type_ref::Type, Error> {
        use representability::convert::{Builtin, Type};
        Ok(match ty {
            Type::Inferred => unreachable!("a crate item's signature names no inferred type"),
            Type::Unit => host::type_ref::Type::unit(),
            Type::Never => host::type_ref::Type::builtin(host::type_ref::Builtin::Never),
            Type::Builtin(builtin) => host::type_ref::Type::builtin(match builtin {
                Builtin::Bool => host::type_ref::Builtin::Bool,
                Builtin::Char => host::type_ref::Builtin::Char,
                Builtin::F64 => host::type_ref::Builtin::F64,
                Builtin::I64 => host::type_ref::Builtin::I64,
                Builtin::InclusiveRange => host::type_ref::Builtin::InclusiveRange,
                Builtin::String | Builtin::Str | Builtin::StaticStr | Builtin::Text => {
                    host::type_ref::Builtin::String
                }
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
                    .expect("a type parameter's index names a declared parameter");
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
                // A program with no host profile: a Rust future is the
                // core's future (D513.3), and std's `Duration` and `Instant`
                // Rawr's own.
                if *definition == rawr_import::surface::identity::Definition::future() {
                    let future =
                        crate::future::facade::definition()?.export_type("future::Future")?;
                    return Ok(host::type_ref::Type::external(future, converted)?);
                }
                // A script closure where a signature names its type, as
                // iced's `Widget::map` gives `Map<Self, F, Message>`: the
                // closure's type, as the input that takes it is (T417).
                if *definition == rawr_import::surface::identity::Definition::callable() {
                    let (output, inputs) = arguments
                        .split_last()
                        .expect("a type parameter's index names a declared parameter");
                    let input = match inputs {
                        [] => host::type_ref::Type::unit(),
                        [single] => Self::convert_charged(single, parameters)?,
                        many => host::type_ref::Type::tuple(
                            many.iter()
                                .map(|ty| Self::convert_charged(ty, parameters))
                                .collect::<Result<Vec<_>, _>>()?,
                        )?,
                    };
                    return Ok(host::type_ref::Type::callable(
                        input,
                        Self::convert_charged(output, parameters)?,
                    ));
                }
                // std's `PathBuf`, `io::Error` and the others Rawr's std
                // holds as their Rust values: Rawr's std's own type.
                if let Some(path) =
                    rawr_import::surface::identity::Definition::std_path(definition.as_bytes())
                {
                    let std = crate::std_facade::definition().export_type(path)?;
                    return Ok(host::type_ref::Type::external(std, converted)?);
                }
                if *definition == rawr_import::surface::identity::Definition::duration()
                    && converted.is_empty()
                {
                    return Ok(host::type_ref::Type::builtin(
                        host::type_ref::Builtin::Duration,
                    ));
                }
                if *definition == rawr_import::surface::identity::Definition::instant()
                    && converted.is_empty()
                {
                    return Ok(host::type_ref::Type::builtin(
                        host::type_ref::Builtin::Instant,
                    ));
                }
                // core's `RangeInclusive<T>`: Rawr's range of `T`'s kind of
                // number.
                if *definition == rawr_import::surface::identity::Definition::range_inclusive()
                    && converted.len() == 1
                    && let Some(element) = converted.pop()
                {
                    return Ok(host::type_ref::Type::range_inclusive_of(element));
                }
                // alloc's `Box<T>`: rawr's `Box` (D711 step 5).
                if *definition == rawr_import::surface::identity::Definition::boxed()
                    && converted.len() == 1
                    && let Some(element) = converted.pop()
                {
                    return Ok(host::type_ref::Type::boxed(element));
                }
                // std's map or set: rawr's of its kind (#777).
                if let Some(kind) =
                    rawr_import::surface::identity::Definition::keyed_kind(definition.as_bytes())
                {
                    return Ok(host::type_ref::Type::keyed(
                        kind,
                        std::mem::take(&mut converted).into_boxed_slice(),
                    ));
                }
                // alloc's `Rc<T>` or `Arc<T>` of a crate's type: rawr's,
                // standing for the crate's own pointer (#775).
                if let Some(arc) =
                    rawr_import::surface::identity::Definition::counter(definition.as_bytes())
                    && converted.len() == 1
                    && let Some(element) = converted.pop()
                {
                    return Ok(host::type_ref::Type::counted(arc, element));
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
            Type::Borrowed { mutable, ty } => {
                host::type_ref::Type::reference(*mutable, Self::convert_charged(ty, parameters)?)
            }
            Type::Range(element) => {
                host::type_ref::Type::range(Self::convert_charged(element, parameters)?)
            }
            Type::Result(ok, err) => host::type_ref::Type::result(
                Self::convert_charged(ok, parameters)?,
                Self::convert_charged(err, parameters)?,
            ),
            Type::Sequence {
                element,
                length: Some(length),
                ..
            } => host::type_ref::Type::array(
                Self::convert_charged(element, parameters)?,
                usize::try_from(*length).expect("an array length fits in usize"),
            ),
            // A crate's `[T]`, whose elements a call copies, or its `Vec<T>`.
            Type::Sequence {
                element,
                owned: false,
                ..
            } => host::type_ref::Type::slice(Self::convert_charged(element, parameters)?),
            Type::Sequence { element, .. } => {
                host::type_ref::Type::vec(Self::convert_charged(element, parameters)?)
            }
            Type::Tuple(members) => host::type_ref::Type::tuple(
                members
                    .iter()
                    .map(|member| Self::convert_charged(member, parameters))
                    .collect::<Result<Vec<_>, _>>()?,
            )?,
            // A script type's proxy fills a call's instance; no Rust
            // declaration names one (D594).
            Type::Script { .. } => unreachable!("no crate declaration names a program's type"),
        })
    }
}

/// What the type that fills an imported type parameter must implement: the
/// derivable std traits its bounds name, which a script type filling it as
/// `Value` is checked for (review 3). `Copy` and `Eq` bring the traits they
/// extend.
pub(crate) fn requirements(bounds: &[Box<str>]) -> Vec<capability::Kind> {
    let mut kinds = Vec::new();
    for name in bounds {
        let Some(kind) = capability::Kind::from_derive(name) else {
            continue;
        };
        let implied = match kind {
            capability::Kind::Copy => Some(capability::Kind::Clone),
            capability::Kind::Eq => Some(capability::Kind::PartialEq),
            _ => None,
        };
        for kind in [Some(kind), implied].into_iter().flatten() {
            if !kinds.contains(&kind) {
                kinds.push(kind);
            }
        }
    }
    kinds
}

fn signature(
    types: &Types,
    signature: &representability::signature::Signature,
    allocation: &mut rawr_import::contract::Allocation,
) -> Result<host::signature::Definition, Error> {
    signature_with(types, signature, None, allocation)
}

/// `signature`, with `receiver` as its first input when given.
fn signature_with(
    types: &Types,
    signature: &representability::signature::Signature,
    receiver: Option<host::type_ref::Type>,
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
        if parameter.kind() == representability::signature::Kind::Type {
            builder.type_parameter(parameter.name(), &requirements(parameter.bounds()))?;
        }
    }
    if let Some(receiver) = receiver {
        builder.try_parameter(receiver)?;
    }
    for input in signature.inputs() {
        builder.try_parameter(types.convert_input(input, signature.parameters(), allocation)?)?;
    }
    builder.output(types.convert(signature.output(), signature.parameters(), allocation)?);
    if let Some(instance) = fixed_instance(types, signature, allocation)? {
        builder.instance(instance);
    }
    relations(&mut builder, signature)?;
    Ok(builder.finish())
}

/// The signature's relations, each a relation of `builder`'s.
fn relations(
    builder: &mut host::signature::Builder,
    signature: &representability::signature::Signature,
) -> Result<(), Error> {
    let parameters = signature.parameters();
    for source in signature.relations() {
        let Some(parameter) = parameters
            .iter()
            .find(|parameter| parameter.index() == source.parameter())
        else {
            continue;
        };
        if let Some(relation) = relation(
            parameter.name(),
            source.arguments(),
            source
                .cases()
                .iter()
                .map(|case| (case.this(), case.arguments())),
            source.trait_identity(),
            source
                .associated()
                .map(|associated| (associated.trait_identity(), associated.name())),
            |ty| converted_pattern(ty, parameters),
        ) {
            builder.relation(relation)?;
        }
    }
    Ok(())
}

/// A relation on `parameter`, its types made patterns by `pattern`: `None`
/// when one of its trait arguments can't be one. A case that can't be is
/// dropped, and a relation without cases, a trait or an associated type is
/// none.
pub(crate) fn relation<'a, T: 'a>(
    parameter: &str,
    arguments: &'a [T],
    cases: impl Iterator<Item = (&'a T, &'a [T])>,
    trait_identity: Option<[u8; 32]>,
    associated: Option<([u8; 32], &str)>,
    pattern: impl Fn(&T) -> Option<host::signature::Pattern>,
) -> Option<host::signature::Relation> {
    let arguments = arguments
        .iter()
        .map(&pattern)
        .collect::<Option<Box<[_]>>>()?;
    let cases = cases
        .filter_map(|(this, implemented)| {
            Some(host::signature::Case::new(
                pattern(this)?,
                implemented
                    .iter()
                    .map(&pattern)
                    .collect::<Option<Box<[_]>>>()?,
            ))
        })
        .collect::<Box<[_]>>();
    (!cases.is_empty() || trait_identity.is_some() || associated.is_some()).then(|| {
        host::signature::Relation::new(parameter, arguments, cases)
            .of_trait(trait_identity)
            .of_associated(associated.map(|(identity, name)| (identity, name.into())))
    })
}

/// A classified type as a relation's pattern.
fn converted_pattern(
    ty: &representability::convert::Type,
    parameters: &[representability::signature::Parameter],
) -> Option<host::signature::Pattern> {
    use host::signature::Pattern;
    use representability::convert::Type;
    use representability::signature::IMPL_PARAMETER;
    Some(match ty {
        Type::Unit => Pattern::Unit,
        Type::Parameter(index) if index & IMPL_PARAMETER != 0 => {
            Pattern::Variable(index & !IMPL_PARAMETER)
        }
        Type::Parameter(index) => Pattern::Parameter(
            parameters
                .iter()
                .find(|parameter| parameter.index() == *index)?
                .name()
                .into(),
        ),
        Type::Builtin(builtin) => {
            Pattern::Builtin(host::type_ref::Builtin::primitive(builtin.rust_name()?)?)
        }
        Type::Range(element) => Pattern::Range(Box::new(converted_pattern(element, parameters)?)),
        Type::Option(element) => Pattern::Option(Box::new(converted_pattern(element, parameters)?)),
        Type::Tuple(members) => Pattern::Tuple(
            members
                .iter()
                .map(|member| converted_pattern(member, parameters))
                .collect::<Option<_>>()?,
        ),
        Type::Nominal { .. }
        | Type::Sequence {
            length: Some(_), ..
        } if !names_impl_parameter(ty) => {
            Pattern::Type(Types::convert_charged(ty, parameters).ok()?)
        }
        // A crate type that names the impl's own parameters, as `Text<'a,
        // Theme, Renderer>` of `From<Text<..>> for Element<..>`: each is
        // named as the case's variable.
        Type::Nominal { .. } => {
            let mut indices = Vec::new();
            impl_parameters(ty, &mut indices);
            let mut named = parameters.to_vec();
            for index in indices {
                named.push(representability::signature::Parameter::new(
                    index,
                    host::signature::case_variable(index & !IMPL_PARAMETER).into(),
                    representability::signature::Kind::Type,
                ));
            }
            Pattern::Generic(Types::convert_charged(ty, &named).ok()?)
        }
        _ => return None,
    })
}

/// The indexes of the implementation's own type parameters `ty` names.
fn impl_parameters(ty: &representability::convert::Type, found: &mut Vec<u32>) {
    use representability::convert::Type;
    match ty {
        Type::Parameter(index) if index & representability::signature::IMPL_PARAMETER != 0 => {
            if !found.contains(index) {
                found.push(*index);
            }
        }
        Type::Nominal { arguments, .. }
        | Type::Tuple(arguments)
        | Type::Script { arguments, .. } => {
            for argument in arguments {
                impl_parameters(argument, found);
            }
        }
        Type::Option(element)
        | Type::Borrowed { ty: element, .. }
        | Type::Range(element)
        | Type::Sequence { element, .. } => impl_parameters(element, found),
        Type::Result(ok, err) => {
            impl_parameters(ok, found);
            impl_parameters(err, found);
        }
        _ => {}
    }
}

/// Whether `ty` names an implementation's own type parameter.
fn names_impl_parameter(ty: &representability::convert::Type) -> bool {
    use representability::convert::Type;
    match ty {
        Type::Parameter(index) => index & representability::signature::IMPL_PARAMETER != 0,
        Type::Inferred => false,
        Type::Unit | Type::Never | Type::Builtin(_) => false,
        Type::Nominal { arguments, .. }
        | Type::Tuple(arguments)
        | Type::Script { arguments, .. } => arguments.iter().any(names_impl_parameter),
        Type::Option(element)
        | Type::Borrowed { ty: element, .. }
        | Type::Range(element)
        | Type::Sequence { element, .. } => names_impl_parameter(element),
        Type::Result(ok, err) => names_impl_parameter(ok) || names_impl_parameter(err),
        Type::FunctionItem { input, output } => {
            names_impl_parameter(input) || names_impl_parameter(output)
        }
        Type::ProfileMessage { payload, .. } => names_impl_parameter(payload),
    }
}

/// The signature of a generic item whose callable parameter takes a script
/// closure: that parameter is the closure's type, `Fn(..) -> R` of its
/// bound's inputs and output (D513.7 b).
fn callable_signature(
    types: &Types,
    signature: &representability::signature::Signature,
    call: Option<&representability::callable::Call>,
    crossings: &[representability::callable::Call],
    allocation: &mut rawr_import::contract::Allocation,
) -> Result<host::signature::Definition, Error> {
    if call.is_none() && crossings.is_empty() {
        return self::signature(types, signature, allocation);
    }
    let mut builder = host::signature::Builder::new();
    for parameter in signature.parameters() {
        if parameter.kind() == representability::signature::Kind::Type {
            builder.type_parameter(parameter.name(), &requirements(parameter.bounds()))?;
        }
    }
    // A script function fills each callable parameter as a slot (D594).
    if !crossings.is_empty() {
        builder.crossing();
    }
    let convert = |ty: &representability::convert::Type,
                   allocation: &mut rawr_import::contract::Allocation| {
        types.convert(ty, signature.parameters(), allocation)
    };
    let closure = |call: &representability::callable::Call,
                   allocation: &mut rawr_import::contract::Allocation|
     -> Result<host::type_ref::Type, Error> {
        let input = match call.inputs() {
            [] => host::type_ref::Type::unit(),
            [single] => convert(single, allocation)?,
            many => host::type_ref::Type::tuple(
                many.iter()
                    .map(|ty| convert(ty, allocation))
                    .collect::<Result<Vec<_>, _>>()?,
            )?,
        };
        Ok(host::type_ref::Type::callable(
            input,
            convert(call.output(), allocation)?,
        ))
    };
    for input in signature.inputs() {
        let found = crossings
            .iter()
            .chain(call)
            .find(|call| *input == representability::convert::Type::Parameter(call.parameter()));
        // An `Option` of the closure Rawr makes, as `on_toggle_maybe` takes.
        let optional = call.filter(|call| {
            matches!(input, representability::convert::Type::Option(inner)
                if **inner == representability::convert::Type::Parameter(call.parameter()))
        });
        if let Some(call) = found {
            builder.try_parameter(closure(call, allocation)?)?;
        } else if let Some(call) = optional {
            builder.try_parameter(host::type_ref::Type::option(closure(call, allocation)?))?;
        } else {
            builder.try_parameter(types.convert_input(
                input,
                signature.parameters(),
                allocation,
            )?)?;
        }
    }
    builder.output(convert(signature.output(), allocation)?);
    if let Some(instance) = fixed_instance(types, signature, allocation)? {
        builder.instance(instance);
    }
    // A closure's input the receiver fixes, as `P::State` of
    // `.subscription(|state| ..)`, as the facade's signature keeps them.
    relations(&mut builder, signature)?;
    Ok(builder.finish())
}

/// The receiver's type a method's impl fixes, when one of its arguments
/// holds the impl's own parameter, as `impl<T> Task<Option<T>>` fixes
/// `Task<Option<T>>`: which form of an overloaded method a receiver takes.
fn fixed_instance(
    types: &Types,
    signature: &representability::signature::Signature,
    allocation: &mut rawr_import::contract::Allocation,
) -> Result<Option<host::type_ref::Type>, Error> {
    use representability::convert::Type;
    fn names_parameter(ty: &Type) -> bool {
        match ty {
            Type::Parameter(_) => true,
            Type::Nominal { arguments, .. }
            | Type::Tuple(arguments)
            | Type::Script { arguments, .. } => arguments.iter().any(names_parameter),
            Type::Option(element)
            | Type::Borrowed { ty: element, .. }
            | Type::Range(element)
            | Type::Sequence { element, .. } => names_parameter(element),
            Type::Result(ok, err) => names_parameter(ok) || names_parameter(err),
            _ => false,
        }
    }
    let Some(instance @ Type::Nominal { arguments, .. }) = signature.instance() else {
        return Ok(None);
    };
    if !arguments
        .iter()
        .any(|argument| !matches!(argument, Type::Parameter(_)) && names_parameter(argument))
    {
        return Ok(None);
    }
    Ok(Some(types.convert(
        instance,
        signature.parameters(),
        allocation,
    )?))
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
                .expect("a type parameter's index names a declared parameter");
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
        Type::ProfileMessage { payload, .. }
        | Type::Option(payload)
        | Type::Borrowed { ty: payload, .. }
        | Type::Range(payload)
        | Type::Sequence {
            element: payload, ..
        } => type_usage(payload, parameters)?,
        Type::Tuple(members)
        | Type::Script {
            arguments: members, ..
        } => members
            .iter()
            .try_fold((0_usize, 0_usize), |(items, bytes), member| {
                let (member_items, member_bytes) = type_usage(member, parameters)?;
                Ok::<_, Error>((
                    items.checked_add(member_items).ok_or(Error::Allocation)?,
                    bytes.checked_add(member_bytes).ok_or(Error::Allocation)?,
                ))
            })?,
        Type::Inferred => (0, 0),
        Type::Unit | Type::Never | Type::Builtin(_) => (0, 0),
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

/// The traits a crate implements for a type, in the order `copy`,
/// `default`, `debug`, `clone`, `partial_eq`, `eq` and `hash`, as the
/// capabilities they give it.
pub(crate) fn implemented(traits: [bool; 10]) -> Vec<crate::capability::Kind> {
    use crate::capability::Kind;
    [
        Kind::Copy,
        Kind::Default,
        Kind::Debug,
        Kind::Clone,
        Kind::PartialEq,
        Kind::Eq,
        Kind::Hash,
        Kind::PartialOrd,
        Kind::Ord,
        Kind::Iterator,
    ]
    .into_iter()
    .zip(traits)
    .filter_map(|(kind, implemented)| implemented.then_some(kind))
    .collect()
}

/// Each parameter's default type, or none when no parameter has one.
fn defaults(
    types: &Types,
    parameters: &[representability::signature::Parameter],
    allocation: &mut rawr_import::contract::Allocation,
) -> Result<Box<[Option<host::type_ref::Type>]>, Error> {
    if parameters
        .iter()
        .all(|parameter| parameter.default().is_none())
    {
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

/// How a method's borrow guard output is reached, when it gives one.
fn guard(signature: &representability::signature::Signature) -> Option<host::item::Guard> {
    signature.guard().map(|guard| match guard {
        representability::signature::Guard::Shared => host::item::Guard::Shared,
        representability::signature::Guard::Mutable => host::item::Guard::Mutable,
        representability::signature::Guard::OptionShared => host::item::Guard::OptionShared,
        representability::signature::Guard::OptionMutable => host::item::Guard::OptionMutable,
    })
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

#[cfg(test)]
mod tests {
    use super::Error;

    /// What a program meets here speaks about its files and the crate items
    /// it names, never about rawr's snapshots, selectors or ledgers (#523).
    #[test]
    fn messages_name_the_program() {
        let cases = [
            (
                Error::SnapshotMismatch,
                "a source file changed during the build; run it again",
            ),
            (
                Error::UnmappedImportedUse {
                    path: "iced::widget::text".into(),
                    selector: rawr_import::surface::identity::Selector::Unavailable,
                },
                "`iced::widget::text` is not supported in Rawr yet",
            ),
            (
                Error::Allocation,
                "out of memory while reading the program's dependencies",
            ),
        ];
        for (error, expected) in cases {
            assert_eq!(error.to_string(), expected);
        }
    }

    /// A crate item's use the build of the dependencies can't take is
    /// reported at that use, naming the item, and never as a "surface
    /// instance" (#565).
    #[test]
    fn an_unsupported_crate_use_reports_at_its_span() {
        use rawr_import::surface::Error as Surface;
        let text = "fn main() { modal(base, content, hide); }";
        let start = text.find("modal").unwrap();
        let span = crate::Span::new(
            crate::Position::new(1, start as u16 + 1),
            crate::Position::new(1, start as u16 + 6),
        );
        for (error, expected) in [
            (
                Surface::InstanceMismatch,
                "calling `iced::widget::stack` with these arguments isn't supported yet",
            ),
            (
                Surface::UnconstrainedGeneric,
                "this use of `iced::widget::stack` leaves one of its type parameters \
                 unknown, which isn't supported yet",
            ),
        ] {
            let report =
                super::unsupported_use(error, "iced::widget::stack", "main.rs", text, span)
                    .report()
                    .expect("the use's report");
            assert_eq!(report.message(), expected);
            assert_eq!(report.labels().len(), 1);
            assert!(!report.message().contains("surface"));
        }
        // A failure that isn't the use's stays the build's own.
        assert!(matches!(
            super::unsupported_use(Surface::Allocation, "a::b", "main.rs", text, span).report(),
            Ok(report) if !report.message().contains("calling")
        ));
    }
}
