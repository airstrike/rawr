//! Immutable compiler proof bundle pairing stable metadata with lowering.

use crate::{capability, host, item, resolve, signature, type_def};
use crate::{Engine, ImmutableString, Scope, AST};
use std::collections::BTreeMap;
use std::fmt;
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

/// One compiler-validated program paired with its exact AST and support.
///
/// Construction is private and consumes one resolved program. There is no API
/// that accepts caller-supplied metadata or an [`AST`], so lowering from one
/// compilation cannot be paired with proofs from another.
///
/// ```compile_fail
/// use rawr::{program::Executable, AST};
/// let _ = Executable::new(AST::empty());
/// ```
///
/// ```compile_fail
/// use rawr::Engine;
/// let executable = Engine::new().compile_executable("40 + 2").unwrap();
/// let _ = executable.ast;
/// ```
pub struct Executable {
    origin: super::origin::Origin,
    package: ImmutableString,
    #[cfg_attr(feature = "no_std", allow(dead_code))]
    source: super::source::Id,
    sources: super::source::Table,
    generation: super::GenerationId,
    #[cfg_attr(feature = "no_function", allow(dead_code))]
    model: super::model::Model,
    schemas: super::schema::Manifest,
    callables: BTreeMap<item::Key, Callable>,
    application: Option<crate::application::Entry>,
    previews: Box<[crate::preview::Definition]>,
    widgets: Box<[crate::widget::Definition]>,
    widget_promotions: Box<[crate::widget::Promotion]>,
    preview_fingerprint: crate::preview::Fingerprint,
    host_dependencies: Box<[host::facade::Dependency]>,
    runtime_requirements: Box<[host::runtime::Requirement]>,
    projection_requirements: Box<[host::projection::Site]>,
    widget_requirements: Box<[host::runtime::widget::Requirement]>,
    /// Source evidence of compiled functions that are not callables (the
    /// functions of trait impls for builtin types), by source: an
    /// error inside one, reached from a callable, still names user source.
    helper_locations: BTreeMap<super::source::Key, Box<[Location]>>,
    ast: AST,
    /// The same contribution also follows closure-captured compiler support.
    #[cfg(feature = "rust-import")]
    imported_owner: Option<crate::package::import::runner::account::support::Owner>,
}

/// Compiler-only application metadata with no lowered program or runtime authority.
#[cfg(feature = "rust-import")]
pub(crate) struct ContractEvidence {
    source: super::source::Id,
    callables: BTreeMap<item::Key, Callable>,
    application: Option<crate::application::Entry>,
    preview_fingerprint: crate::preview::Fingerprint,
    host_dependencies: Box<[host::facade::Dependency]>,
}

pub(crate) trait ApplicationEvidence {
    #[cfg(feature = "rust-import")]
    fn application(&self) -> Option<&crate::application::Entry>;
    fn callable(&self, key: &item::Key) -> Option<&Callable>;
    fn preview_fingerprint(&self) -> &crate::preview::Fingerprint;
    fn host_dependencies(&self) -> &[host::facade::Dependency];
}

#[cfg_attr(feature = "no_function", allow(dead_code))]
pub(crate) struct Callable {
    #[cfg_attr(feature = "no_std", allow(dead_code))]
    source: super::source::Id,
    value: item::Value,
    arity: usize,
    /// A free function's `&mut` parameter its body takes as `this`
    /// (`fn update(state: &mut State, ..)`), bound as a method's receiver.
    this: Option<usize>,
    dispatch: Dispatch,
    application: Option<crate::application::descriptor::Signature>,
    locations: Option<Locations>,
}

pub(crate) struct Evidence<'a> {
    key: &'a item::Key,
    source: &'a crate::diagnostic::Source,
    declaration: crate::Span,
    locations: Option<&'a Locations>,
    helpers: Option<&'a [Location]>,
}

struct Locations {
    source: super::source::Key,
    entries: Box<[Location]>,
}

#[derive(Clone, Copy)]
struct Location {
    generated: crate::Span,
    original: crate::Span,
    nesting: usize,
}

/// The compiler's evidence for one program's source, kept apart from its
/// [`AST`]: it maps a runtime error's position in the lowered program back
/// to the source the program was compiled from.
#[doc(hidden)]
pub struct Positions {
    callables: Box<[Location]>,
    helpers: Box<[Location]>,
}

impl Positions {
    /// The source span of `error`'s deepest position. It is `None` when the
    /// compiler kept no evidence for that position.
    #[must_use]
    pub fn translate(&self, error: &crate::EvalAltResult) -> Option<crate::Span> {
        let position = deepest_position(error);
        if position.is_none() {
            return None;
        }
        innermost(&self.callables, position).or_else(|| innermost(&self.helpers, position))
    }
}

/// The original span of the innermost generated span holding `position`.
fn innermost(entries: &[Location], position: crate::Position) -> Option<crate::Span> {
    entries
        .iter()
        .filter(|location| {
            position >= location.generated.start() && position <= location.generated.end()
        })
        .min_by_key(|location| {
            (
                location.nesting,
                span_width(location.generated),
                location.generated.start(),
                location.original.start(),
                location.original.end(),
            )
        })
        .map(|location| location.original)
}

#[cfg_attr(feature = "no_function", allow(dead_code))]
pub(crate) enum Dispatch {
    Function(ImmutableString),
    Method(ImmutableString),
    Variant {
        identity: type_def::Identity,
        schema: super::schema::Fingerprint,
        variant: crate::types::nominal::variant::Id,
    },
}

impl fmt::Debug for Executable {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Executable")
            .field("types", &self.schemas.len())
            .field("items", &self.callables.len())
            .field("lowering", &"<private>")
            .finish()
    }
}

impl Executable {
    pub(crate) fn from_resolved(
        engine: &Engine,
        program: resolve::Program,
        scope: Option<&Scope>,
        #[cfg(not(feature = "no_optimize"))] optimization_level: crate::OptimizationLevel,
    ) -> crate::parser::ParseResult<Self> {
        Self::from_resolved_at(
            engine,
            program,
            scope,
            #[cfg(not(feature = "no_optimize"))]
            optimization_level,
        )
        .map_err(resolve::LoweringFailure::into_error)
    }

    #[cfg(not(feature = "no_std"))]
    pub(crate) fn from_package_resolved(
        engine: &Engine,
        program: resolve::Program,
        scope: Option<&Scope>,
        #[cfg(not(feature = "no_optimize"))] optimization_level: crate::OptimizationLevel,
    ) -> Result<Self, resolve::LoweringFailure> {
        Self::from_resolved_at(
            engine,
            program,
            scope,
            #[cfg(not(feature = "no_optimize"))]
            optimization_level,
        )
    }

    fn from_resolved_at(
        engine: &Engine,
        program: resolve::Program,
        scope: Option<&Scope>,
        #[cfg(not(feature = "no_optimize"))] optimization_level: crate::OptimizationLevel,
    ) -> Result<Self, resolve::LoweringFailure> {
        Self::from_lowering(engine, program, |program, model| {
            resolve::lower_resolved(
                engine,
                program,
                model,
                scope,
                #[cfg(not(feature = "no_optimize"))]
                optimization_level,
            )
            .map(|output| (output, ()))
        })
        .map(|(executable, ())| executable)
    }

    /// Consume one resolved program into executable support and its native-use evidence.
    #[cfg(all(not(feature = "no_function"), feature = "rust-import"))]
    pub(crate) fn from_package_with_uses<A: host::ImportAllocation>(
        engine: &Engine,
        program: resolve::Program,
        allocation: &mut A,
        scope: Option<&Scope>,
        #[cfg(not(feature = "no_optimize"))] optimization_level: crate::OptimizationLevel,
    ) -> Result<(Self, resolve::lower_nominal::ImportedOutput), resolve::ImportedFailure> {
        let owner = allocation.support_owner();
        let (mut executable, imported) = Self::from_lowering(engine, program, |program, model| {
            resolve::lower_nominal::with_uses(
                engine,
                program,
                model,
                allocation,
                scope,
                #[cfg(not(feature = "no_optimize"))]
                optimization_level,
            )
        })?;
        executable.imported_owner = owner;
        Ok((executable, imported))
    }

    fn from_lowering<T, E>(
        engine: &Engine,
        program: resolve::Program,
        lower: impl FnOnce(
            &resolve::Program,
            &super::model::Model,
        ) -> Result<(resolve::lower_nominal::Output, T), E>,
    ) -> Result<(Self, T), E> {
        let origin = engine.origin();
        let package = program.package().into();
        let source = program.source().entry();
        let sources = program.source().table().clone();
        let generation = super::GenerationId::issue();
        let model = super::model::Model::new(generation, &program);
        let schemas = super::schema::Manifest::from_program(&program);
        let mut callables = collect_callables(&program);
        let application = program
            .application()
            .map(|entry| collect_application(entry, &callables, &program));
        let previews = program.previews().to_vec().into_boxed_slice();
        let widgets = program.widgets().to_vec().into_boxed_slice();
        let preview_fingerprint = crate::preview::Fingerprint::from_definitions(&previews);
        let (lowering, evidence) = lower(&program, &model)?;
        for closure in lowering.closures {
            insert_callable(
                &mut callables,
                closure.value.key().clone(),
                Callable {
                    source: closure.source,
                    value: closure.value,
                    arity: closure.arity,
                    this: None,
                    dispatch: Dispatch::Function(closure.dispatch),
                    application: None,
                    locations: None,
                },
            );
        }
        let helper_locations = retain_locations(&mut callables, &sources, lowering.locations);

        Ok((
            Self {
                origin,
                package,
                source,
                sources,
                generation,
                model,
                schemas,
                callables,
                application,
                previews,
                widgets,
                widget_promotions: lowering.widget_promotions,
                preview_fingerprint,
                host_dependencies: lowering.dependencies,
                runtime_requirements: lowering.runtime_requirements,
                projection_requirements: lowering.projection_requirements,
                widget_requirements: lowering.widget_requirements,
                helper_locations,
                ast: lowering.ast,
                #[cfg(feature = "rust-import")]
                imported_owner: None,
            },
            evidence,
        ))
    }

    /// Return this executable's opaque generation identifier.
    #[must_use]
    pub const fn generation(&self) -> super::GenerationId {
        self.generation
    }

    pub(crate) const fn origin(&self) -> super::origin::Origin {
        self.origin
    }

    pub(crate) fn package(&self) -> &str {
        &self.package
    }

    #[cfg_attr(feature = "no_function", allow(dead_code))]
    pub(crate) const fn model(&self) -> &super::model::Model {
        &self.model
    }

    /// Return stable symbolic values for every callable declared by the program.
    pub fn items(&self) -> impl ExactSizeIterator<Item = &item::Value> {
        self.callables.values().map(|callable| &callable.value)
    }

    /// Return the number of stable nominal type definitions in this program.
    #[must_use]
    pub const fn type_count(&self) -> usize {
        self.schemas.len()
    }

    /// Return the stable nominal schema manifest sealed into this executable.
    #[must_use]
    pub const fn schemas(&self) -> &super::schema::Manifest {
        &self.schemas
    }

    /// Return the number of stable symbolic callable definitions.
    #[must_use]
    pub fn item_count(&self) -> usize {
        self.callables.len()
    }

    pub(crate) fn callable(&self, key: &item::Key) -> Option<&Callable> {
        self.callables.get(key)
    }

    pub(crate) const fn application(&self) -> Option<&crate::application::Entry> {
        self.application.as_ref()
    }

    /// Return every compiler-validated local preview in stable key order.
    pub const fn previews(&self) -> &[crate::preview::Definition] {
        &self.previews
    }

    /// Return every compiler-validated scripted-widget protocol.
    pub const fn widgets(&self) -> &[crate::widget::Definition] {
        &self.widgets
    }

    /// Return every compiler-selected nominal-widget promotion site.
    pub const fn widget_promotions(&self) -> &[crate::widget::Promotion] {
        &self.widget_promotions
    }

    pub(crate) const fn preview_fingerprint(&self) -> &crate::preview::Fingerprint {
        &self.preview_fingerprint
    }

    pub(crate) const fn host_dependencies(&self) -> &[host::facade::Dependency] {
        &self.host_dependencies
    }

    pub(crate) const fn runtime_requirements(&self) -> &[host::runtime::Requirement] {
        &self.runtime_requirements
    }

    pub(crate) const fn projection_requirements(&self) -> &[host::projection::Site] {
        &self.projection_requirements
    }

    pub(crate) const fn widget_requirements(&self) -> &[host::runtime::widget::Requirement] {
        &self.widget_requirements
    }

    pub(crate) fn preview_source(
        &self,
        location: crate::preview::Location,
    ) -> Result<&crate::diagnostic::Source, super::source::Error> {
        self.sources.resolve(location.source())
    }

    /// Consume this executable by requiring its compiler-selected application root.
    pub fn require_application(
        self,
    ) -> Result<crate::application::Candidate, crate::application::Error> {
        crate::application::Candidate::from_executable(self)
    }

    /// Consume this executable by requiring at least one validated preview.
    pub fn require_previews(self) -> Result<crate::preview::Catalog, crate::preview::Error> {
        crate::preview::Catalog::from_executable(self)
    }

    pub(crate) fn related_callable(&self, key: &item::Key) -> Option<&Callable> {
        let mut candidates = self
            .callables
            .iter()
            .filter(|(candidate, _)| candidate.same_unowned_path(key));
        let first = candidates.next()?.1;
        candidates.next().is_none().then_some(first)
    }

    pub(crate) fn callable_evidence<'a>(
        &'a self,
        callable: &'a Callable,
    ) -> Result<Evidence<'a>, super::source::Error> {
        let location = self.sources.key(callable.source.index());
        let source = self.sources.resolve(location)?;
        let locations = match callable.locations.as_ref() {
            Some(locations) => {
                self.sources.resolve(locations.source)?;
                (locations.source == location).then_some(locations)
            }
            None => None,
        };
        let declaration = callable
            .application_signature()
            .map(crate::application::descriptor::Signature::declaration)
            .unwrap_or(crate::Span::NONE);
        Ok(Evidence {
            key: callable.value.key(),
            source,
            declaration,
            locations,
            helpers: self
                .helper_locations
                .get(&location)
                .map(|entries| &entries[..]),
        })
    }

    #[cfg_attr(feature = "no_function", allow(dead_code))]
    pub(crate) const fn ast(&self) -> &AST {
        &self.ast
    }

    pub(crate) fn into_ast(self) -> AST {
        self.ast
    }

    /// This program's [`AST`] and the [`Positions`] of its entry source.
    pub(crate) fn into_ast_with_positions(self) -> (AST, Positions) {
        let entry = self.sources.key(self.source.index());
        let callables = self
            .callables
            .values()
            .filter_map(|callable| callable.locations.as_ref())
            .filter(|locations| locations.source == entry)
            .flat_map(|locations| locations.entries.iter().copied())
            .collect();
        let helpers = self
            .helper_locations
            .get(&entry)
            .map(|entries| entries.clone())
            .unwrap_or_default();
        (self.ast, Positions { callables, helpers })
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn retain_imported(
        &mut self,
        completed: crate::package::import::runner::account::support::Completed,
    ) {
        completed.require(
            self.imported_owner
                .as_ref()
                .expect("private imported lowering retains its original support owner"),
        );
    }
}

#[cfg(feature = "rust-import")]
impl ContractEvidence {
    pub(crate) fn from_resolved(
        program: &resolve::Program,
        host_dependencies: Box<[host::facade::Dependency]>,
    ) -> Self {
        let callables = collect_callables(program);
        let application = program
            .application()
            .map(|entry| collect_application(entry, &callables, &program));
        Self {
            source: program.source().entry(),
            callables,
            application,
            preview_fingerprint: crate::preview::Fingerprint::from_definitions(program.previews()),
            host_dependencies,
        }
    }

    #[cfg(not(feature = "no_std"))]
    pub(crate) fn application_error_source(
        &self,
        error: &crate::application::Error,
    ) -> super::source::Id {
        application_error_source(self.source, self.application(), &self.callables, error)
    }
}

impl ApplicationEvidence for Executable {
    #[cfg(feature = "rust-import")]
    fn application(&self) -> Option<&crate::application::Entry> {
        self.application()
    }

    fn callable(&self, key: &item::Key) -> Option<&Callable> {
        self.callable(key)
    }

    fn preview_fingerprint(&self) -> &crate::preview::Fingerprint {
        self.preview_fingerprint()
    }

    fn host_dependencies(&self) -> &[host::facade::Dependency] {
        self.host_dependencies()
    }
}

#[cfg(feature = "rust-import")]
impl ApplicationEvidence for ContractEvidence {
    fn application(&self) -> Option<&crate::application::Entry> {
        self.application.as_ref()
    }

    fn callable(&self, key: &item::Key) -> Option<&Callable> {
        self.callables.get(key)
    }

    fn preview_fingerprint(&self) -> &crate::preview::Fingerprint {
        &self.preview_fingerprint
    }

    fn host_dependencies(&self) -> &[host::facade::Dependency] {
        &self.host_dependencies
    }
}

#[cfg_attr(feature = "no_function", allow(dead_code))]
impl Callable {
    pub(crate) const fn value(&self) -> &item::Value {
        &self.value
    }

    pub(crate) const fn arity(&self) -> usize {
        self.arity
    }

    pub(crate) const fn this(&self) -> Option<usize> {
        self.this
    }

    pub(crate) const fn dispatch(&self) -> &Dispatch {
        &self.dispatch
    }

    pub(crate) const fn application_signature(
        &self,
    ) -> Option<&crate::application::descriptor::Signature> {
        self.application.as_ref()
    }
}

fn collect_callables(program: &resolve::Program) -> BTreeMap<item::Key, Callable> {
    let definitions: BTreeMap<_, _> = program
        .types()
        .iter()
        .map(|definition| (definition.id(), definition))
        .collect();
    let application_runtime = crate::application::RuntimeGraph::new(program);
    let mut callables = BTreeMap::new();

    for shell in program.items() {
        match &shell.kind {
            resolve::item::Kind::Function(function) => {
                if program.application().is_some()
                    && shell.source == program.source().entry()
                    && function.name.text == "main"
                {
                    continue;
                }
                let key = shell
                    .key
                    .clone()
                    .expect("resolved top-level functions retain their stable item key");
                let value = item::Value::new(
                    key.clone(),
                    item::Kind::Function,
                    None,
                    None,
                    signature::Fingerprint::function(
                        false,
                        None,
                        &function.parameters,
                        function.output.as_ref(),
                    ),
                    capability::Fingerprint::default(),
                );
                insert_callable(
                    &mut callables,
                    key.clone(),
                    Callable {
                        source: shell.source,
                        value,
                        arity: function.parameters.len(),
                        this: crate::resolve::lower_nominal::this_parameter(function),
                        dispatch: Dispatch::Function(if program.source().is_aggregate() {
                            crate::resolve::lower_nominal::private_function_name(&key)
                        } else {
                            function.name.text.clone()
                        }),
                        application: Some(application_function_signature(
                            function,
                            &definitions,
                            &application_runtime,
                        )),
                        locations: None,
                    },
                );
            }
            resolve::item::Kind::Enum(item) => {
                let definition = definitions[&item.definition];
                let owner = type_def::Identity::Script(definition.key().clone());
                let capabilities = definition.capabilities().fingerprint();

                for variant in &item.variants {
                    let key = item::Key::associated(
                        definition.key(),
                        variant.name.text.clone(),
                        item::Kind::VariantConstructor,
                    );
                    let value = item::Value::new(
                        key.clone(),
                        item::Kind::VariantConstructor,
                        Some(owner.clone()),
                        None,
                        signature::Fingerprint::variant(&variant.payload, owner.clone()),
                        capabilities.clone(),
                    );
                    insert_callable(
                        &mut callables,
                        key,
                        Callable {
                            source: shell.source,
                            value,
                            arity: variant.payload.len(),
                            this: None,
                            dispatch: Dispatch::Variant {
                                identity: owner.clone(),
                                schema: definition.schema().clone(),
                                variant: crate::types::nominal::variant::Id::from_name(
                                    &variant.name,
                                ),
                            },
                            application: None,
                            locations: None,
                        },
                    );
                }
            }
            resolve::item::Kind::Use(_)
            | resolve::item::Kind::Struct(_)
            | resolve::item::Kind::Impl(_)
            | resolve::item::Kind::Trait(_)
            | resolve::item::Kind::LegacyFunction(_) => {}
        }
    }

    for definition in program.methods() {
        let owner = definition.owner_identity().clone();
        let kind = if definition.receiver().is_some() {
            item::Kind::Method
        } else {
            item::Kind::AssociatedFunction
        };
        let capabilities = definitions[&definition.owner()]
            .capabilities()
            .fingerprint();
        let value = item::Value::new(
            definition.key().clone(),
            kind,
            Some(owner),
            definition.receiver(),
            definition.fingerprint().clone(),
            capabilities,
        );
        insert_callable(
            &mut callables,
            definition.key().clone(),
            Callable {
                source: definition.source(),
                value,
                arity: definition.parameters().len() + usize::from(definition.receiver().is_some()),
                this: None,
                dispatch: Dispatch::Method(definition.symbol()),
                application: Some(application_method_signature(
                    definition,
                    definitions[&definition.owner()],
                    &definitions,
                    &application_runtime,
                )),
                locations: None,
            },
        );
    }

    callables
}

/// Attach each callable's source evidence to it; return the evidence of
/// compiled functions that are not callables, by source.
fn retain_locations(
    callables: &mut BTreeMap<item::Key, Callable>,
    sources: &super::source::Table,
    locations: Box<[crate::resolve::lower_nominal::Location]>,
) -> BTreeMap<super::source::Key, Box<[Location]>> {
    let mut grouped: BTreeMap<item::Key, (super::source::Id, Vec<Location>)> = BTreeMap::new();
    let mut helpers: BTreeMap<super::source::Key, Vec<Location>> = BTreeMap::new();
    for location in locations {
        let Some(callable) = callables.get(&location.callable) else {
            helpers
                .entry(sources.key(location.source.index()))
                .or_default()
                .push(Location {
                    generated: location.generated,
                    original: location.original,
                    nesting: location.nesting,
                });
            continue;
        };
        if callable.source != location.source {
            continue;
        }
        grouped
            .entry(location.callable)
            .or_insert_with(|| (location.source, Vec::new()))
            .1
            .push(Location {
                generated: location.generated,
                original: location.original,
                nesting: location.nesting,
            });
    }
    for (key, (source, entries)) in grouped {
        if let Some(callable) = callables.get_mut(&key) {
            callable.locations = Some(Locations {
                source: sources.key(source.index()),
                entries: entries.into_boxed_slice(),
            });
        }
    }
    helpers
        .into_iter()
        .map(|(source, entries)| (source, entries.into_boxed_slice()))
        .collect()
}

impl Evidence<'_> {
    pub(crate) const fn source(&self) -> &crate::diagnostic::Source {
        self.source
    }

    pub(crate) const fn declaration(&self) -> crate::Span {
        self.declaration
    }

    pub(crate) fn translate(&self, error: &super::Error) -> Option<crate::Span> {
        let super::Error::Evaluation { key, error } = error else {
            return None;
        };
        if key != self.key {
            return None;
        }
        let position = deepest_position(error);
        // The callable's own body, else a builtin-type impl function it
        // called.
        self.locations
            .and_then(|locations| innermost(&locations.entries, position))
            .or_else(|| self.helpers.and_then(|helpers| innermost(helpers, position)))
    }
}

fn span_width(span: crate::Span) -> (usize, usize) {
    (
        span.end()
            .line()
            .unwrap_or(usize::MAX)
            .saturating_sub(span.start().line().unwrap_or(0)),
        span.end()
            .position()
            .unwrap_or(usize::MAX)
            .saturating_sub(span.start().position().unwrap_or(0)),
    )
}

fn deepest_position(error: &crate::EvalAltResult) -> crate::Position {
    match error {
        crate::EvalAltResult::ErrorInFunctionCall(.., inner, outer)
        | crate::EvalAltResult::ErrorInModule(.., inner, outer) => {
            deepest_position(inner).or_else(*outer)
        }
        leaf => leaf.position(),
    }
}

#[cfg(not(feature = "no_std"))]
impl Executable {
    pub(crate) fn application_error_source(
        &self,
        error: &crate::application::Error,
    ) -> super::source::Id {
        application_error_source(
            self.model_source(),
            self.application(),
            &self.callables,
            error,
        )
    }

    pub(crate) fn model_source(&self) -> super::source::Id {
        self.source
    }
}

#[cfg(not(feature = "no_std"))]
fn application_error_source(
    fallback: super::source::Id,
    entry: Option<&crate::application::Entry>,
    callables: &BTreeMap<item::Key, Callable>,
    error: &crate::application::Error,
) -> super::source::Id {
    let Some(entry) = entry else {
        return fallback;
    };
    let callback = match error {
        crate::application::Error::InvalidCallbackKind { role, .. }
        | crate::application::Error::InvalidReceiver { role, .. }
        | crate::application::Error::InvalidArity { role, .. }
        | crate::application::Error::InvalidOutput { role, .. }
        | crate::application::Error::TypeMismatch { role, .. }
        | crate::application::Error::UnsupportedRuntimeType { role, .. } => {
            if role.contains("boot") {
                entry.boot()
            } else if role.contains("title") {
                entry.title()
            } else if role.contains("view") {
                Some(entry.view())
            } else {
                Some(entry.update())
            }
        }
        crate::application::Error::MissingDefault { .. }
        | crate::application::Error::MissingCapability { .. } => Some(entry.update()),
        _ => None,
    };
    callback
        .and_then(|callback| callables.get(callback.key()))
        .map_or(fallback, |callable| callable.source)
}

fn application_function_signature(
    function: &resolve::item::Function,
    definitions: &BTreeMap<type_def::Id, &type_def::Definition>,
    runtime: &crate::application::RuntimeGraph,
) -> crate::application::descriptor::Signature {
    let output = function
        .output
        .as_ref()
        .map(|output| crate::application::Type::from_resolved(output, definitions, runtime))
        .unwrap_or_else(|| {
            crate::application::Type::from_resolved(
                &crate::resolve::type_ref::Type::Unit(function.body.span),
                definitions,
                runtime,
            )
        });
    crate::application::descriptor::Signature::new(
        None,
        None,
        function
            .parameters
            .iter()
            .map(|parameter| {
                crate::application::Type::from_resolved(&parameter.ty, definitions, runtime)
            })
            .collect(),
        output,
        function.name.span,
        None,
        function
            .parameters
            .iter()
            .map(|parameter| parameter.span)
            .collect(),
        function
            .output
            .as_ref()
            .map_or(function.body.span, resolved_type_span),
    )
}

fn application_method_signature(
    method: &crate::method::Definition,
    owner: &type_def::Definition,
    definitions: &BTreeMap<type_def::Id, &type_def::Definition>,
    runtime: &crate::application::RuntimeGraph,
) -> crate::application::descriptor::Signature {
    crate::application::descriptor::Signature::new(
        Some(crate::application::Type::from_definition(owner, runtime)),
        method.receiver(),
        method
            .parameters()
            .iter()
            .map(|parameter| {
                crate::application::Type::from_resolved(&parameter.ty, definitions, runtime)
            })
            .collect(),
        crate::application::Type::from_resolved(method.output(), definitions, runtime),
        method.name_span(),
        method.receiver_span(),
        method
            .parameters()
            .iter()
            .map(|parameter| parameter.span)
            .collect(),
        resolved_type_span(method.output()),
    )
}

fn resolved_type_span(ty: &crate::resolve::type_ref::Type) -> crate::Span {
    match ty {
        crate::resolve::type_ref::Type::Unit(span)
        | crate::resolve::type_ref::Type::Tuple { span, .. }
        | crate::resolve::type_ref::Type::Reference { span, .. } => *span,
        crate::resolve::type_ref::Type::Path(path) => path.span,
    }
}

fn collect_application(
    entry: &resolve::application::Entry,
    callables: &BTreeMap<item::Key, Callable>,
    program: &resolve::Program,
) -> crate::application::Entry {
    // The statements before the entry, as written, so a runner can tell
    // an edit to them.
    let prelude = entry.prelude().map(|key| {
        let text = entry
            .prelude_span(program.items())
            .zip(
                program
                    .items()
                    .iter()
                    .find(|shell| shell.key.as_ref() == Some(key)),
            )
            .map(|(span, shell)| span_text(program.source().text_at(shell.source), span))
            .unwrap_or_default();
        (
            callables
                .get(key)
                .map(|callable| callable.value.clone())
                .expect("the statements before the entry retain their executable item"),
            text,
        )
    });
    let value = |key: &item::Key| {
        callables
            .get(key)
            .map(|callable| callable.value.clone())
            .expect("resolved application callbacks retain stable executable items")
    };

    crate::application::Entry::new(
        entry.form(),
        entry.dependency().clone(),
        entry.root_intrinsic().clone(),
        entry.title_intrinsic().cloned(),
        entry.theme_intrinsic().cloned(),
        entry.subscription_intrinsic().cloned(),
        entry.centered_intrinsic().cloned(),
        entry.run_intrinsic().cloned(),
        entry.boot().map(value),
        prelude,
        value(entry.update()),
        value(entry.view()),
        entry.title().map(value),
        entry.theme().map(value),
        entry.subscription().map(value),
        entry.centered(),
        entry.task().cloned(),
        entry.theme_type().cloned(),
        entry.subscription_type().cloned(),
        entry.widget().cloned(),
        entry.span(),
    )
}

/// The text of `source` within `span`.
fn span_text(source: &str, span: crate::Span) -> Box<str> {
    let mut text = String::new();
    let (mut line, mut column) = (1_u16, 0_u16);
    for character in source.chars() {
        if character == '\n' {
            line = line.saturating_add(1);
            column = 0;
        } else {
            column = column.saturating_add(1);
        }
        let position = crate::Position::new(line, column);
        if position >= span.start() && position < span.end() {
            text.push(character);
        }
    }
    text.into()
}

fn insert_callable(
    callables: &mut BTreeMap<item::Key, Callable>,
    key: item::Key,
    callable: Callable,
) {
    assert!(
        callables.insert(key, callable).is_none(),
        "resolved stable callable keys are unique"
    );
}
