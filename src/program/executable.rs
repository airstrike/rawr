//! Immutable compiler proof bundle pairing stable metadata with lowering.

use crate::{capability, host, item, resolve, signature, type_def};
use crate::{Engine, ImmutableString, Scope, AST};
use std::collections::BTreeMap;
use std::fmt;

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
    source: super::source::Id,
    sources: super::source::Table,
    generation: super::GenerationId,
    model: super::model::Model,
    schemas: super::schema::Manifest,
    callables: BTreeMap<item::Key, Callable>,
    host_dependencies: Box<[host::facade::Dependency]>,
    runtime_requirements: Box<[host::runtime::Requirement]>,
    projection_requirements: Box<[host::projection::Site]>,
    /// Source evidence of compiled functions that are not callables (the
    /// functions of trait impls for builtin types), by source: an
    /// error inside one, reached from a callable, still names user source.
    helper_locations: BTreeMap<super::source::Key, Box<[Location]>>,
    ast: AST,
    /// The same contribution also follows closure-captured compiler support.
    #[cfg(feature = "rust-import")]
    imported_owner: Option<crate::package::import::runner::account::support::Owner>,
}

pub(crate) struct Callable {
    source: super::source::Id,
    value: item::Value,
    arity: usize,
    /// A free function's `&mut` parameter its body takes as `this`
    /// (`fn update(state: &mut State, ..)`), bound as a method's receiver.
    this: Option<usize>,
    dispatch: Dispatch,
    /// A free function's signature as written, for typed handles.
    signature: Option<super::signatures::Signature>,
    locations: Option<Locations>,
    /// A trait impl's function for a builtin or a crate's type, which other
    /// functions call as a helper too: its source evidence is theirs.
    helper: bool,
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
    /// The program's other sources, each with its own evidence: where
    /// [`Self::locate`] looks when the entry source holds no position.
    others: Box<[(crate::diagnostic::Source, Positions)]>,
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
        self.original(position)
    }

    /// The source and span of `error`'s deepest position: in the entry
    /// source, `entry`, or in another source of the program, such as a
    /// module's file. It is `None` when the compiler kept no evidence for
    /// that position.
    #[must_use]
    pub fn locate<'a>(
        &'a self,
        entry: &'a crate::diagnostic::Source,
        error: &crate::EvalAltResult,
    ) -> Option<(&'a crate::diagnostic::Source, crate::Span)> {
        self.locate_position(entry, deepest_position(error))
    }

    /// The source and span that `position` in the lowered program comes
    /// from: in the entry source, `entry`, or in another source of the
    /// program. It is `None` in code the compiler wrote around the
    /// sources'.
    #[must_use]
    pub fn locate_position<'a>(
        &'a self,
        entry: &'a crate::diagnostic::Source,
        position: crate::Position,
    ) -> Option<(&'a crate::diagnostic::Source, crate::Span)> {
        if position.is_none() {
            return None;
        }
        self.original(position)
            .map(|span| (entry, span))
            .or_else(|| {
                self.others.iter().find_map(|(source, positions)| {
                    positions.original(position).map(|span| (source, span))
                })
            })
    }

    /// `position` in the lowered program as a position in the source's own
    /// text, or none when the evidence does not cover it: code the compiler
    /// wrote around the source's.
    pub(crate) fn located(&self, position: crate::Position) -> crate::Position {
        if position.is_none() {
            return position;
        }
        self.original(position)
            .map_or(crate::Position::NONE, |span| span.start())
    }

    fn original(&self, position: crate::Position) -> Option<crate::Span> {
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
        optimization_level: crate::OptimizationLevel,
    ) -> crate::parser::ParseResult<Self> {
        Self::from_resolved_at(engine, program, scope, optimization_level)
            .map_err(resolve::LoweringFailure::into_error)
    }

    pub(crate) fn from_package_resolved(
        engine: &Engine,
        program: resolve::Program,
        scope: Option<&Scope>,
        optimization_level: crate::OptimizationLevel,
    ) -> Result<Self, resolve::LoweringFailure> {
        Self::from_resolved_at(engine, program, scope, optimization_level)
    }

    fn from_resolved_at(
        engine: &Engine,
        program: resolve::Program,
        scope: Option<&Scope>,
        optimization_level: crate::OptimizationLevel,
    ) -> Result<Self, resolve::LoweringFailure> {
        Self::from_lowering(engine, program, |program, model| {
            resolve::lower_resolved(engine, program, model, scope, optimization_level)
                .map(|output| (output, ()))
        })
        .map(|(executable, ())| executable)
    }

    /// Consume one resolved program into executable support and its native-use evidence.
    #[cfg(feature = "rust-import")]
    pub(crate) fn from_package_with_uses<A: host::ImportAllocation>(
        engine: &Engine,
        program: resolve::Program,
        allocation: &mut A,
        scope: Option<&Scope>,
        optimization_level: crate::OptimizationLevel,
    ) -> Result<(Self, resolve::lower_nominal::ImportedOutput), resolve::ImportedFailure> {
        let mut program = program;
        resolve::lower_nominal::reveal(
            engine,
            &mut program,
            &engine.host_registry.share(),
            allocation,
        )?;
        let owner = allocation.support_owner();
        let (mut executable, imported) = Self::from_lowering(engine, program, |program, model| {
            resolve::lower_nominal::with_uses(
                engine,
                program,
                model,
                allocation,
                scope,
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
                    signature: None,
                    locations: None,
                    helper: false,
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
                host_dependencies: lowering.dependencies,
                runtime_requirements: lowering.runtime_requirements,
                projection_requirements: lowering.projection_requirements,
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

    pub(crate) const fn host_dependencies(&self) -> &[host::facade::Dependency] {
        &self.host_dependencies
    }

    pub(crate) const fn runtime_requirements(&self) -> &[host::runtime::Requirement] {
        &self.runtime_requirements
    }

    // A candidate is made only by a rust-import build.
    #[cfg_attr(not(feature = "rust-import"), allow(dead_code))]
    pub(crate) const fn projection_requirements(&self) -> &[host::projection::Site] {
        &self.projection_requirements
    }

    pub(crate) fn related_callable(&self, key: &item::Key) -> Option<&Callable> {
        let mut candidates = self
            .callables
            .iter()
            .filter(|(candidate, _)| candidate.same_unowned_path(key));
        let first = candidates.next()?.1;
        candidates.next().is_none().then_some(first)
    }

    pub(crate) const fn ast(&self) -> &AST {
        &self.ast
    }

    /// This program's [`AST`], which keeps the facade items it calls so
    /// its first evaluation binds them (D494 item 15).
    pub(crate) fn into_hosted_ast(self) -> AST {
        let mut ast = self.ast;
        if !self.host_dependencies.is_empty() {
            ast.set_host(super::hosted::Hosted::new(
                self.origin,
                self.host_dependencies,
                self.runtime_requirements,
                self.projection_requirements,
            ));
        }
        ast
    }

    /// This program's hosted [`AST`] and the typed signatures of its entry
    /// source's free functions, which typed handles check against (D514).
    pub(crate) fn into_hosted_parts(mut self) -> (AST, super::signatures::Signatures) {
        let mut signatures = super::signatures::Signatures::default();
        let entry = self.source;
        for callable in std::mem::take(&mut self.callables).into_values() {
            if callable.source == entry
                && let Dispatch::Function(_) = callable.dispatch
                && let Some(signature) = callable.signature
            {
                signatures.insert(signature);
            }
        }
        (self.into_hosted_ast(), signatures)
    }

    /// This program's [`AST`] and the [`Positions`] of its entry source.
    pub(crate) fn into_ast_with_positions(self) -> (AST, Positions) {
        let positions = self.positions();
        (self.into_hosted_ast(), positions)
    }

    /// This program, compiled from one file whose inline modules are its
    /// other sources, as the single-file compiler gives it: each source's
    /// evidence is the entry's, since an inline module keeps the file's
    /// positions, and each of the root's free functions is callable by its
    /// own name.
    pub(crate) fn into_one_file(mut self) -> Self {
        let entry = self.sources.key(self.source.index());
        for locations in self
            .callables
            .values_mut()
            .filter_map(|callable| callable.locations.as_mut())
        {
            locations.source = entry;
        }
        let helpers = std::mem::take(&mut self.helper_locations)
            .into_values()
            .flat_map(Vec::from)
            .collect::<Box<[_]>>();
        if !helpers.is_empty() {
            self.helper_locations.insert(entry, helpers);
        }
        for (key, callable) in &self.callables {
            if callable.source == self.source
                && key.modules().is_empty()
                && let Dispatch::Function(name) = &callable.dispatch
                && callable.signature.is_some()
            {
                self.ast.alias_script_function(name, key.name());
            }
        }
        self
    }

    /// This program's entry source and the [`Positions`] of every source,
    /// for a debugger to place the program's steps.
    #[cfg(all(feature = "debugging", feature = "rust-import"))]
    pub(crate) fn located(&self) -> Option<crate::inspect::Located> {
        let entry = self.sources.key(self.source.index());
        let entry = self.sources.resolve(entry).ok()?.clone();
        Some(crate::inspect::Located::new(entry, self.positions()))
    }

    /// The [`Positions`] of this program's entry source, with each other
    /// source's own.
    pub(crate) fn positions(&self) -> Positions {
        let entry = self.sources.key(self.source.index());
        let mut keys: Vec<_> = self
            .callables
            .values()
            .filter_map(|callable| callable.locations.as_ref())
            .map(|locations| locations.source)
            .chain(self.helper_locations.keys().copied())
            .filter(|key| *key != entry)
            .collect();
        keys.sort_unstable();
        keys.dedup();
        let others = keys
            .into_iter()
            .filter_map(|key| {
                let source = self.sources.resolve(key).ok()?.clone();
                Some((source, self.source_positions(key, Box::default())))
            })
            .collect();
        self.source_positions(entry, others)
    }

    /// The [`Positions`] of the source `key`.
    fn source_positions(
        &self,
        key: super::source::Key,
        others: Box<[(crate::diagnostic::Source, Positions)]>,
    ) -> Positions {
        let callables = self
            .callables
            .values()
            .filter_map(|callable| callable.locations.as_ref())
            .filter(|locations| locations.source == key)
            .flat_map(|locations| locations.entries.iter().copied())
            .collect();
        let helpers = self.helper_locations.get(&key).cloned().unwrap_or_default();
        Positions {
            callables,
            helpers,
            others,
        }
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
}

fn collect_callables(program: &resolve::Program) -> BTreeMap<item::Key, Callable> {
    let definitions: BTreeMap<_, _> = program
        .types()
        .iter()
        .map(|definition| (definition.id(), definition))
        .collect();
    let mut callables = BTreeMap::new();

    for shell in program.items() {
        match &shell.kind {
            resolve::item::Kind::Function(function) => {
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
                        signature: Some(super::signatures::Signature::from_resolved(function)),
                        locations: None,
                        helper: false,
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
                            signature: None,
                            locations: None,
                            helper: false,
                        },
                    );
                }
            }
            resolve::item::Kind::Use(_)
            | resolve::item::Kind::Struct(_)
            | resolve::item::Kind::Impl(_)
            | resolve::item::Kind::Trait(_) => {}
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
                signature: None,
                locations: None,
                helper: false,
            },
        );
    }

    // A trait impl's function for a crate's type, as `from` of `impl
    // From<Shade> for iced::Color`: native code calls it through a runner's
    // impl, as it calls a script type's (D594).
    for definition in program.builtin_methods() {
        if definition.trait_name().is_none() || definitions.contains_key(&definition.owner()) {
            continue;
        }
        let kind = if definition.receiver().is_some() {
            item::Kind::Method
        } else {
            item::Kind::AssociatedFunction
        };
        let value = item::Value::new(
            definition.key().clone(),
            kind,
            Some(definition.owner_identity().clone()),
            definition.receiver(),
            definition.fingerprint().clone(),
            crate::capability::Set::new().fingerprint(),
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
                signature: None,
                locations: None,
                helper: true,
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
        let Some((key, callable)) = location
            .callable
            .as_ref()
            .and_then(|key| Some((key.clone(), callables.get(key)?)))
        else {
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
        if callable.helper {
            helpers
                .entry(sources.key(location.source.index()))
                .or_default()
                .push(Location {
                    generated: location.generated,
                    original: location.original,
                    nesting: location.nesting,
                });
        }
        grouped
            .entry(key)
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
