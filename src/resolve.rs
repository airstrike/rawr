//! Promotion of parsed Rawr units into opaque resolved programs.

pub(crate) mod application;
mod error;
mod intrinsic;
pub(crate) mod item;
pub(crate) mod lower_nominal;
#[cfg(feature = "rust-import")]
pub(crate) mod native;
#[cfg(feature = "std")]
pub(crate) mod package;
pub(crate) mod type_ref;

use crate::{host, method, parse, program, type_def, ImmutableString, Span};
pub use error::{Category, Error};
use std::collections::BTreeMap;

pub(crate) struct LocatedError {
    primary: program::source::Id,
    secondary: Option<program::source::Id>,
    error: Error,
}

impl LocatedError {
    pub(crate) const fn primary(primary: program::source::Id, error: Error) -> Self {
        Self {
            primary,
            secondary: None,
            error,
        }
    }

    pub(crate) const fn related(
        primary: program::source::Id,
        secondary: program::source::Id,
        error: Error,
    ) -> Self {
        Self {
            primary,
            secondary: Some(secondary),
            error,
        }
    }

    fn into_error(self) -> Error {
        self.error
    }

    #[cfg(feature = "std")]
    fn into_parts(self) -> (program::source::Id, Option<program::source::Id>, Error) {
        (self.primary, self.secondary, self.error)
    }
}

/// A complete, successfully resolved Rawr compilation unit.
///
/// Fields and construction remain private so an unresolved or partially
/// resolved unit cannot masquerade as an executable candidate.
///
/// ```compile_fail
/// let _ = rawr::resolve::Program {};
/// ```
pub struct Program {
    package: ImmutableString,
    source: program::Source,
    items: Box<[item::Shell]>,
    types: Box<[type_def::Definition]>,
    methods: Box<[method::Definition]>,
    /// The functions of trait impls for builtin types (`impl Shape for
    /// i64`): lowered with the methods, but not program members.
    builtin_methods: Box<[method::Definition]>,
    application: Option<application::Entry>,
    previews: Box<[crate::preview::Definition]>,
    widgets: Box<[crate::widget::Definition]>,
    preview_paths: BTreeMap<(program::source::Id, Span), crate::preview::PathTarget>,
    qualified_functions: BTreeMap<(program::source::Id, Span), crate::item::Key>,
    qualified_owners: BTreeMap<(program::source::Id, Span), type_def::Identity>,
    qualified_pattern_owners: BTreeMap<(program::source::Id, Span), type_def::Identity>,
    /// Qualified expression paths into a `.rs` module, by path span,
    /// spelled through the app's crate name.
    qualified_rust: BTreeMap<(program::source::Id, Span), Box<[ImmutableString]>>,
    /// The app crate's name, when a marked root's `async fn main` names a
    /// runtime's entry, which the shadow crate holds (D513.4).
    entry_crate: Option<ImmutableString>,
    /// The type aliases each source names, declared or imported, which a
    /// body reads as the item headers do.
    aliases: BTreeMap<(program::source::Id, ImmutableString), type_ref::Alias>,
    scripts: Box<[Script]>,
    #[cfg(feature = "rust-import")]
    native: Option<std::sync::Arc<native::Capture>>,
}

pub(crate) struct Script {
    source: program::source::Id,
    /// The statements outside the items, the body of `fn main` to the
    /// checker.
    text: String,
}

pub(crate) struct LoweringFailure {
    evidence: LoweringEvidence,
    error: crate::ParseError,
}

#[derive(Debug)]
pub(crate) enum LoweringEvidence {
    Original(program::source::Id),
    Generated {
        source: program::source::Id,
        span: Span,
    },
    Unmapped(ImmutableString),
}

impl LoweringFailure {
    fn original(source: program::source::Id, error: crate::ParseError) -> Self {
        Self {
            evidence: LoweringEvidence::Original(source),
            error,
        }
    }

    fn generated(source: program::source::Id, span: Span, error: crate::ParseError) -> Self {
        Self {
            evidence: LoweringEvidence::Generated { source, span },
            error,
        }
    }

    fn unmapped(reason: impl Into<ImmutableString>, error: crate::ParseError) -> Self {
        Self {
            evidence: LoweringEvidence::Unmapped(reason.into()),
            error,
        }
    }

    pub(crate) fn into_parts(self) -> (LoweringEvidence, crate::ParseError) {
        (self.evidence, self.error)
    }

    pub(crate) fn into_error(self) -> crate::ParseError {
        match self.evidence {
            LoweringEvidence::Original(_) => self.error,
            LoweringEvidence::Generated { span, .. } => {
                crate::ParseError(self.error.0, span.start(), self.error.2)
            }
            LoweringEvidence::Unmapped(_) => {
                crate::ParseError(self.error.0, crate::Position::NONE, self.error.2)
            }
        }
    }
}

impl Program {
    pub(crate) fn package(&self) -> &str {
        &self.package
    }

    /// Access the source promoted into this resolved program.
    #[must_use]
    pub const fn source(&self) -> &program::Source {
        &self.source
    }

    /// Return the number of resolved top-level item shells.
    #[must_use]
    pub const fn item_count(&self) -> usize {
        self.items.len()
    }

    /// Return the number of nominal type definitions in this generation.
    #[must_use]
    pub const fn type_count(&self) -> usize {
        self.types.len()
    }

    /// Return the number of resolved inherent methods in this generation.
    #[must_use]
    pub const fn method_count(&self) -> usize {
        self.methods.len()
    }

    #[allow(dead_code)]
    pub(crate) const fn items(&self) -> &[item::Shell] {
        &self.items
    }

    #[allow(dead_code)]
    pub(crate) const fn types(&self) -> &[type_def::Definition] {
        &self.types
    }

    #[allow(dead_code)]
    pub(crate) const fn methods(&self) -> &[method::Definition] {
        &self.methods
    }

    pub(crate) const fn application(&self) -> Option<&application::Entry> {
        self.application.as_ref()
    }

    pub(crate) const fn previews(&self) -> &[crate::preview::Definition] {
        &self.previews
    }

    pub(crate) const fn widgets(&self) -> &[crate::widget::Definition] {
        &self.widgets
    }

    pub(crate) fn preview_path(
        &self,
        source: program::source::Id,
        span: Span,
    ) -> Option<&crate::preview::PathTarget> {
        self.preview_paths.get(&(source, span))
    }
}

#[cfg(feature = "rust-import")]
pub(crate) enum ImportedFailure {
    Lowering(LoweringFailure),
    Surface(rawr_import::surface::Error),
}

#[cfg(feature = "rust-import")]
impl ImportedFailure {
    #[cfg(test)]
    pub(crate) fn into_error(self) -> crate::ParseError {
        match self {
            Self::Lowering(failure) => failure.into_error(),
            Self::Surface(error) => {
                panic!("unexpected allocation failure in compiler test: {error}")
            }
        }
    }
}

#[cfg(feature = "rust-import")]
pub(crate) fn lower_imported(
    engine: &crate::Engine,
    program: &Program,
    host: &host::Registry,
    allocation: &mut rawr_import::contract::Allocation,
) -> Result<lower_nominal::ImportedOutput, ImportedFailure> {
    lower_nominal::imported(engine, program, host, allocation)
}

/// Why programs in this process may read no file, once denied.
static NO_FILES: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();

/// Stop programs in this process from reading files. `include_str!` and
/// `include_bytes!` then fail with `reason`, and the iced facade draws
/// each image or svg path as a missing file. `?play` calls it, since its
/// snippets run on a server whose files are not theirs.
pub fn deny_files(reason: &'static str) {
    let _ = NO_FILES.set(reason);
}

/// Why programs in this process may read no file, when [`deny_files`] was
/// called.
#[must_use]
pub fn files_denied() -> Option<&'static str> {
    NO_FILES.get().copied()
}

/// Resolve all item declarations and header types in a parsed unit.
///
/// This is the only transformation that can construct a [`Program`].
pub fn resolve(unit: parse::Unit, host: &host::Registry) -> Result<Program, Error> {
    let evidence = crate::diagnostic::Source::new("<memory>", unit.source().text())
        .expect("the compiler-owned in-memory source identity is valid");
    resolve_with_evidence(unit, host, evidence)
}

pub(crate) fn resolve_with_evidence(
    unit: parse::Unit,
    host: &host::Registry,
    evidence: crate::diagnostic::Source,
) -> Result<Program, Error> {
    resolve_with_evidence_and_namespace(
        unit,
        host,
        &Namespace::root(),
        AllocationOrder::Source,
        evidence,
    )
}

fn resolve_with_evidence_and_namespace(
    unit: parse::Unit,
    host: &host::Registry,
    namespace: &Namespace,
    allocation_order: AllocationOrder,
    evidence: crate::diagnostic::Source,
) -> Result<Program, Error> {
    let (source, parsed_items, _, statement_source) = unit.into_parts();
    if let Some(module) = parsed_items.iter().find_map(|item| match item {
        crate::item::Parsed::Module(module) => Some(module),
        crate::item::Parsed::Use(_)
        | crate::item::Parsed::Struct(_)
        | crate::item::Parsed::Enum(_)
        | crate::item::Parsed::Impl(_)
        | crate::item::Parsed::Trait(_)
        | crate::item::Parsed::Function(_)
        | crate::item::Parsed::TypeAlias(_) => None,
    }) {
        return Err(Error::UnsupportedNominalSyntax {
            construct: "file modules require project/package compilation".into(),
            source: module.span,
        });
    }
    let symbols = Symbols::predeclare(&parsed_items, namespace, allocation_order, host)?;
    let mut items = Vec::with_capacity(parsed_items.len());

    let aliases = source_aliases(program::source::Id::ROOT, &symbols.types);
    for (source_index, parsed) in parsed_items.into_vec().into_iter().enumerate() {
        if matches!(parsed, crate::item::Parsed::TypeAlias(_)) {
            continue;
        }
        items.push(resolve_item(
            symbols.item_id(source_index),
            program::source::Id::ROOT,
            symbols.item_key(source_index).cloned(),
            symbols.resolved_use(source_index).cloned(),
            parsed,
            &symbols.types,
            host,
            #[cfg(feature = "rust-import")]
            None,
        )?);
    }

    let definitions = symbols.finish(&items).map_err(LocatedError::into_error)?;
    let (methods, builtin_methods) =
        resolve_methods(&mut items, &definitions, host).map_err(LocatedError::into_error)?;
    resolve_functions(&mut items).map_err(LocatedError::into_error)?;
    let application = application::resolve(&mut items, &methods, host, None)?;

    let mut program = Program {
        #[cfg(feature = "rust-import")]
        native: None,
        package: namespace.package.clone(),
        source: program::Source::from_resolved(source, evidence),
        items: items.into_boxed_slice(),
        types: definitions.into_boxed_slice(),
        methods: methods.into_boxed_slice(),
        builtin_methods: builtin_methods.into_boxed_slice(),
        application,
        previews: Box::new([]),
        widgets: Box::new([]),
        preview_paths: BTreeMap::new(),
        qualified_functions: BTreeMap::new(),
        qualified_owners: BTreeMap::new(),
        qualified_pattern_owners: BTreeMap::new(),
        qualified_rust: BTreeMap::new(),
        entry_crate: None,
        aliases,
        scripts: vec![Script {
            source: program::source::Id::ROOT,
            text: statement_source,
        }]
        .into_boxed_slice(),
    };
    validate_invocation_storage(&program).map_err(LocatedError::into_error)?;
    program.widgets = crate::widget::resolve(&program, host).map_err(LocatedError::into_error)?;
    program.previews = crate::preview::resolve(&program, host).map_err(LocatedError::into_error)?;
    Ok(program)
}

pub(crate) fn validate_invocation_storage(program: &Program) -> Result<(), LocatedError> {
    for shell in program.items() {
        let reject = |ty: &type_ref::Type, role: &str| {
            invocation_path(ty).map(|path| {
                LocatedError::primary(
                    shell.source,
                    Error::InvalidHostOperation {
                        item: path.into(),
                        reason: format!(
                            "invocation-scoped host types cannot appear in {role}; they are legal only in an exact facade-authorized invocation formal"
                        )
                        .into(),
                        operation: type_span(ty),
                    },
                )
            })
        };

        match &shell.kind {
            item::Kind::Struct(item) => {
                for field in &item.fields {
                    if let Some(error) = reject(&field.ty, "a nominal field") {
                        return Err(error);
                    }
                }
            }
            item::Kind::Enum(item) => {
                for variant in &item.variants {
                    for payload in &variant.payload {
                        if let Some(error) = reject(payload, "an enum payload") {
                            return Err(error);
                        }
                    }
                }
            }
            item::Kind::Function(function) => {
                for parameter in &function.parameters {
                    if let Some(error) = reject(&parameter.ty, "an ordinary function parameter") {
                        return Err(error);
                    }
                }
                if let Some(error) = function
                    .output
                    .as_ref()
                    .and_then(|output| reject(output, "a function return"))
                {
                    return Err(error);
                }
            }
            item::Kind::Impl(item) => {
                let marked = item
                    .header
                    .attributes
                    .iter()
                    .any(crate::item::Attribute::is_widget);
                for function in &item.functions {
                    for parameter in &function.parameters {
                        if marked
                            && function.name.text == "draw"
                            && invocation_path(&parameter.ty).is_some()
                        {
                            continue;
                        }
                        if let Some(error) = reject(&parameter.ty, "a non-draw method parameter") {
                            return Err(error);
                        }
                    }
                    if let Some(error) = function
                        .output
                        .as_ref()
                        .and_then(|output| reject(output, "a method return"))
                    {
                        return Err(error);
                    }
                }
            }
            item::Kind::Use(_) | item::Kind::Trait(_) => {}
        }
    }
    Ok(())
}

fn direct_mutable_invocation(ty: &type_ref::Type) -> bool {
    let type_ref::Type::Reference {
        mutable: true,
        target,
        ..
    } = ty
    else {
        return false;
    };
    matches!(target.as_ref(), type_ref::Type::Path(path) if matches!(
        &path.target,
        type_ref::Target::Host { definition, .. }
            if definition.storage() == host::type_def::Storage::Invocation
    ))
}

fn invocation_path(ty: &type_ref::Type) -> Option<String> {
    match ty {
        type_ref::Type::Unit(_) => None,
        type_ref::Type::Tuple { members, .. } => members.iter().find_map(invocation_path),
        type_ref::Type::Reference { target, .. } => invocation_path(target),
        type_ref::Type::Path(path) => {
            match &path.target {
                type_ref::Target::Host { definition, .. }
                    if definition.storage() == host::type_def::Storage::Invocation =>
                {
                    return Some(definition.path());
                }
                type_ref::Target::Builtin
                | type_ref::Target::Host { .. }
                | type_ref::Target::Script { .. }
                | type_ref::Target::Parameter
                | type_ref::Target::Projection(..)
                | type_ref::Target::Alias(_) => {}
            }
            path.arguments.iter().find_map(|argument| match argument {
                type_ref::Argument::Type(ty) => invocation_path(ty),
                type_ref::Argument::Lifetime(_) => None,
            })
        }
    }
}

#[cfg(test)]
fn resolve_with(
    unit: parse::Unit,
    host: &host::Registry,
    namespace: &Namespace,
    allocation_order: AllocationOrder,
) -> Result<Program, Error> {
    let evidence = crate::diagnostic::Source::new("<memory>", unit.source().text())
        .expect("the compiler-owned in-memory source identity is valid");
    resolve_with_evidence_and_namespace(unit, host, namespace, allocation_order, evidence)
}

pub(crate) fn lower_with_engine(
    engine: &crate::Engine,
    program: Program,
    scope: Option<&crate::Scope>,
    optimization_level: crate::OptimizationLevel,
) -> crate::parser::ParseResult<program::Executable> {
    for shell in &program.items {
        match &shell.kind {
            item::Kind::Use(_) | item::Kind::Struct(_) | item::Kind::Enum(_) => {}
            item::Kind::Impl(_) | item::Kind::Trait(_) => {}
            item::Kind::Function(function) => reject_unavailable(function)?,
        }
    }

    program::Executable::from_resolved(engine, program, scope, optimization_level)
}

pub(crate) fn lower_package_with_engine(
    engine: &crate::Engine,
    program: Program,
    scope: Option<&crate::Scope>,
    optimization_level: crate::OptimizationLevel,
) -> Result<program::Executable, LoweringFailure> {
    program::Executable::from_package_resolved(engine, program, scope, optimization_level)
}

/// Consume a resolved package into its executable and same-pass imported-use evidence.
#[cfg(feature = "rust-import")]
pub(crate) fn lower_package_with_uses<A: host::ImportAllocation>(
    engine: &crate::Engine,
    program: Program,
    allocation: &mut A,
    scope: Option<&crate::Scope>,
    optimization_level: crate::OptimizationLevel,
) -> Result<(program::Executable, lower_nominal::ImportedOutput), ImportedFailure> {
    program::Executable::from_package_with_uses(
        engine,
        program,
        allocation,
        scope,
        optimization_level,
    )
}

pub(crate) fn lower_resolved(
    engine: &crate::Engine,
    program: &Program,
    model: &program::model::Model,
    scope: Option<&crate::Scope>,
    optimization_level: crate::OptimizationLevel,
) -> Result<lower_nominal::Output, LoweringFailure> {
    lower_nominal::lower(engine, program, model, scope, optimization_level)
}

fn reject_unavailable(function: &item::Function) -> Result<(), Error> {
    match function.body.lowering {
        item::Lowering::Unavailable => Err(Error::LoweringUnavailable {
            item: function.name.text.clone(),
            body: function.body.span,
        }),
        item::Lowering::NominalFunction | item::Lowering::NominalMethod(_) => Ok(()),
    }
}

struct Symbols {
    item_ids: Vec<crate::item::Id>,
    item_keys: Vec<Option<crate::item::Key>>,
    types: BTreeMap<ImmutableString, type_ref::Target>,
    declarations: Vec<type_def::Declaration>,
    uses: Vec<Option<item::Use>>,
}

impl Symbols {
    fn predeclare(
        items: &[crate::item::Parsed],
        namespace: &Namespace,
        allocation_order: AllocationOrder,
        host: &host::Registry,
    ) -> Result<Self, Error> {
        let mut declarations = BTreeMap::<ImmutableString, Span>::new();
        let mut item_ids = vec![None; items.len()];
        let mut item_keys = vec![None; items.len()];
        let mut types = BTreeMap::new();
        let mut type_declarations = Vec::new();

        for parsed in items {
            let Some((name, _, _)) = named_declaration(parsed) else {
                continue;
            };

            if let Some(previous_declaration) = declarations.get(name.text.as_str()) {
                return Err(Error::DuplicateItem {
                    name: name.text.clone(),
                    declaration: name.span,
                    previous_declaration: *previous_declaration,
                });
            }

            declarations.insert(name.text.clone(), name.span);
        }
        for parsed in items {
            let crate::item::Parsed::TypeAlias(alias) = parsed else {
                continue;
            };
            let name = &alias.name;
            if let Some(previous_declaration) = declarations.get(name.text.as_str()) {
                return Err(Error::DuplicateItem {
                    name: name.text.clone(),
                    declaration: name.span,
                    previous_declaration: *previous_declaration,
                });
            }
            declarations.insert(name.text.clone(), name.span);
            types.insert(name.text.clone(), unresolved_alias(alias));
        }

        let mut source_indices: Vec<_> = (0..items.len()).collect();

        if allocation_order == AllocationOrder::Reverse {
            source_indices.reverse();
        }

        for (dense_index, source_index) in source_indices.into_iter().enumerate() {
            let item_id = crate::item::Id::from_index(dense_index);
            item_ids[source_index] = Some(item_id);

            let Some((name, type_kind, item_kind)) = named_declaration(&items[source_index]) else {
                continue;
            };

            item_keys[source_index] = Some(namespace.item_key(&name.text, item_kind));

            if let Some(kind) = type_kind {
                let id = type_def::Id::from_index(type_declarations.len());
                let key = namespace.type_key(&name.text);
                types.insert(
                    name.text.clone(),
                    type_ref::Target::Script {
                        definition: id,
                        identity: type_def::Identity::Script(key.clone()),
                    },
                );
                type_declarations.push(type_def::Declaration::new(
                    id,
                    item_id,
                    key,
                    name.text.clone(),
                    name.span,
                    kind,
                ));
            }
        }

        let uses = resolve_imports(items, host, &declarations, &mut types)?;
        module_header_types(items, &uses, host, &mut types);
        if let Some(error) = resolve_aliases(items, &mut types, host) {
            return Err(error);
        }

        Ok(Self {
            item_ids: item_ids
                .into_iter()
                .map(|id| id.expect("every parsed item receives one generation-local ID"))
                .collect(),
            item_keys,
            types,
            declarations: type_declarations,
            uses,
        })
    }

    fn item_id(&self, source_index: usize) -> crate::item::Id {
        self.item_ids[source_index]
    }

    fn item_key(&self, source_index: usize) -> Option<&crate::item::Key> {
        self.item_keys[source_index].as_ref()
    }

    fn resolved_use(&self, source_index: usize) -> Option<&item::Use> {
        self.uses[source_index].as_ref()
    }

    fn finish(self, items: &[item::Shell]) -> Result<Vec<type_def::Definition>, LocatedError> {
        let mut schemas = BTreeMap::new();
        let capabilities = crate::capability::resolve(items)?;

        for shell in items {
            match &shell.kind {
                item::Kind::Struct(item) => {
                    schemas.insert(item.definition, program::schema::for_struct(item));
                }
                item::Kind::Enum(item) => {
                    schemas.insert(item.definition, program::schema::for_enum(item));
                }
                item::Kind::Use(_)
                | item::Kind::Impl(_)
                | item::Kind::Trait(_)
                | item::Kind::Function(_) => {}
            }
        }

        Ok(self
            .declarations
            .into_iter()
            .map(|declaration| {
                let schema = schemas
                    .remove(&declaration.id())
                    .expect("each predeclared nominal type resolves exactly one schema");
                let capability_set = capabilities[&declaration.id()];
                declaration.resolve(schema, capability_set)
            })
            .collect())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AllocationOrder {
    Source,
    Reverse,
}

#[derive(Clone)]
struct Namespace {
    package: ImmutableString,
    modules: Box<[ImmutableString]>,
}

impl Namespace {
    fn root() -> Self {
        Self {
            package: "main".into(),
            modules: Box::default(),
        }
    }

    #[cfg(test)]
    fn new(package: &str, modules: &[&str]) -> Self {
        Self {
            package: package.into(),
            modules: modules.iter().copied().map(Into::into).collect(),
        }
    }

    fn item_key(&self, name: &str, kind: crate::item::Kind) -> crate::item::Key {
        crate::item::Key::new(
            self.package.clone(),
            self.modules.clone(),
            name.into(),
            kind,
        )
    }

    fn type_key(&self, name: &str) -> type_def::Key {
        type_def::Key::new(self.package.clone(), self.modules.clone(), name.into())
    }
}

fn named_declaration(
    parsed: &crate::item::Parsed,
) -> Option<(
    &crate::item::Name,
    Option<type_def::Kind>,
    crate::item::Kind,
)> {
    match parsed {
        crate::item::Parsed::Module(_)
        | crate::item::Parsed::Use(_)
        | crate::item::Parsed::Impl(_)
        | crate::item::Parsed::Trait(_)
        | crate::item::Parsed::TypeAlias(_) => None,
        crate::item::Parsed::Struct(item) => Some((
            &item.name,
            Some(type_def::Kind::Struct),
            crate::item::Kind::Struct,
        )),
        crate::item::Parsed::Enum(item) => Some((
            &item.name,
            Some(type_def::Kind::Enum),
            crate::item::Kind::Enum,
        )),
        crate::item::Parsed::Function(item) => {
            Some((&item.name, None, crate::item::Kind::Function))
        }
    }
}

/// The type aliases `types` names, for the bodies of `source`.
fn source_aliases(
    source: program::source::Id,
    types: &BTreeMap<ImmutableString, type_ref::Target>,
) -> BTreeMap<(program::source::Id, ImmutableString), type_ref::Alias> {
    types
        .iter()
        .filter_map(|(name, target)| match target {
            type_ref::Target::Alias(alias) => Some(((source, name.clone()), alias.clone())),
            _ => None,
        })
        .collect()
}

/// A type alias before its target resolves.
fn unresolved_alias(alias: &crate::item::TypeAlias) -> type_ref::Target {
    type_ref::Target::Alias(type_ref::Alias {
        params: alias
            .generics
            .iter()
            .map(|generic| generic.name.text.clone())
            .collect(),
        target: None,
    })
}

/// Resolve the target of each type alias `items` declare, in `types`, which
/// names them. An alias may name another alias declared after it, so the
/// targets resolve until none is left or none more resolves. Returns the
/// first error of an alias that does not resolve.
fn resolve_aliases(
    items: &[crate::item::Parsed],
    types: &mut BTreeMap<ImmutableString, type_ref::Target>,
    host: &host::Registry,
) -> Option<Error> {
    let mut pending = items
        .iter()
        .filter_map(|parsed| match parsed {
            crate::item::Parsed::TypeAlias(alias) => Some(alias),
            _ => None,
        })
        .filter(|alias| {
            matches!(
                types.get(&alias.name.text),
                Some(type_ref::Target::Alias(type_ref::Alias {
                    target: None,
                    ..
                }))
            )
        })
        .collect::<Vec<_>>();
    loop {
        let count = pending.len();
        let mut failed = Vec::new();
        let mut first = None;
        for alias in pending {
            let mut scope = types.clone();
            for generic in alias.generics.iter() {
                scope.insert(generic.name.text.clone(), type_ref::Target::Parameter);
            }
            match type_ref::resolve(
                alias.target.clone(),
                &scope,
                host,
                #[cfg(feature = "rust-import")]
                None,
            ) {
                Ok(target) => {
                    if let Some(type_ref::Target::Alias(declared)) = types.get_mut(&alias.name.text)
                    {
                        declared.target = Some(Box::new(target));
                    }
                }
                Err(error) => {
                    first.get_or_insert(error);
                    failed.push(alias);
                }
            }
        }
        if failed.is_empty() {
            return None;
        }
        if failed.len() == count {
            return first;
        }
        pending = failed;
    }
}

fn resolve_imports(
    parsed_items: &[crate::item::Parsed],
    host: &host::Registry,
    local_declarations: &BTreeMap<ImmutableString, Span>,
    types: &mut BTreeMap<ImmutableString, type_ref::Target>,
) -> Result<Vec<Option<item::Use>>, Error> {
    let mut occupied = local_declarations.clone();
    let mut uses = vec![None; parsed_items.len()];
    // A `use` may start at a module another `use` imported, as Rust allows:
    // after `use iced::keyboard;`, `use keyboard::key;` names
    // `iced::keyboard::key`.
    let mut modules = BTreeMap::<ImmutableString, ImmutableString>::new();
    for parsed in parsed_items {
        let crate::item::Parsed::Use(parsed) = parsed else {
            continue;
        };
        for (name, path) in parsed.bindings() {
            let qualified = host.canonical_str(&std_reexport(&path.join("::"), host));
            if matches!(host.entry(&qualified), Some(host::Entry::Module)) {
                modules.insert(name.text, qualified);
            }
        }
    }
    let through_module = |path: ImmutableString| -> ImmutableString {
        if host.entry(&std_reexport(&path, host)).is_some() {
            return path;
        }
        match path.split_once("::") {
            Some((first, rest)) => match modules.get(first) {
                Some(module) => format!("{module}::{rest}").into(),
                None => path,
            },
            None => path,
        }
    };

    for (index, parsed) in parsed_items.iter().enumerate() {
        let crate::item::Parsed::Use(parsed) = parsed else {
            continue;
        };
        let mut bindings = Vec::new();

        let mut globbed = Vec::new();
        if let crate::item::UseTree::Glob { prefix, span } = &parsed.tree {
            let module = parsed_path(prefix);
            for name in std_glob(&module) {
                if !occupied.contains_key(&name) {
                    globbed.push((
                        crate::item::Name {
                            span: *span,
                            text: name.clone(),
                        },
                        format!("{module}::{name}"),
                    ));
                }
            }
        }
        if let crate::item::UseTree::Glob { prefix, span } = &parsed.tree
            && globbed.is_empty()
        {
            return Err(Error::InvalidImport {
                path: parsed_path(prefix),
                reason: "glob imports are supported for Rust modules in a package".into(),
                reference: *span,
                declaration: None,
            });
        }
        // `use ::image::ColorType;` names the crate `image`, never a name
        // of this program or one another `use` brought in.
        let global = parsed.global();
        if global
            && let Some(first) = parsed
                .bindings()
                .first()
                .and_then(|(_, path)| path.first().cloned())
            && (matches!(first.as_str(), "crate" | "self" | "super" | "Self")
                || local_declarations.contains_key(&first))
        {
            return Err(not_a_crate(&first, prefix_span(&parsed.tree)));
        }
        let through_module = |path: ImmutableString| {
            if global {
                path
            } else {
                through_module(path)
            }
        };
        if let crate::item::UseTree::Group { prefix, .. } = &parsed.tree {
            let prefix = through_module(parsed_path(prefix));
            match host.entry(&std_reexport(&prefix, host)) {
                None if std_fmt_import(&prefix) => {}
                Some(host::Entry::Module | host::Entry::ModuleItem(_) | host::Entry::Type(_)) => {}
                Some(host::Entry::Item(_) | host::Entry::Endpoint(_)) => {
                    return Err(Error::InvalidImport {
                        path: prefix,
                        reason: "import groups require a module or type namespace".into(),
                        reference: prefix_span(&parsed.tree),
                        declaration: None,
                    });
                }
                None => {
                    return Err(Error::UnknownImport {
                        path: prefix,
                        reference: prefix_span(&parsed.tree),
                    });
                }
            }
        }
        for (name, path) in parsed.bindings() {
            let qualified = through_module(path.join("::").into());
            let module_only = parsed.binds_module_only(&name);
            let mut binding = resolve_import_binding(name, qualified, host)?;
            if module_only && matches!(binding.target, item::Import::ModuleItem { .. }) {
                binding.target = item::Import::Module;
            }
            bindings.push(binding);
        }
        for (name, qualified) in globbed {
            bindings.push(resolve_import_binding(name, qualified.into(), host)?);
        }

        for binding in &bindings {
            if let Some(previous_declaration) =
                occupied.insert(binding.name.text.clone(), binding.name.span)
            {
                return Err(Error::DuplicateItem {
                    name: binding.name.text.clone(),
                    declaration: binding.name.span,
                    previous_declaration,
                });
            }
            if let item::Import::Type { definition, owner } = &binding.target {
                types.insert(
                    binding.name.text.clone(),
                    type_ref::Target::Host {
                        definition: definition.clone(),
                        owner: owner.clone(),
                    },
                );
            }
        }

        uses[index] = Some(item::Use {
            header: parsed.header.clone(),
            bindings: bindings.into_boxed_slice(),
            span: parsed.span,
        });
    }

    Ok(uses)
}

/// Header types named through an imported host module, as Rust allows:
/// after `use iced::widget::button;`, `button::Status` is a type path.
fn module_header_types(
    parsed_items: &[crate::item::Parsed],
    uses: &[Option<item::Use>],
    host: &host::Registry,
    types: &mut BTreeMap<ImmutableString, type_ref::Target>,
) {
    let mut paths = Vec::new();
    for parsed in parsed_items {
        package::collect_item_type_paths(parsed, &mut paths);
    }
    for path in paths {
        let [first, ..] = &*path.segments else {
            continue;
        };
        let name = parsed_path(path);
        if path.segments.len() < 2 || path.global || types.contains_key(&name) {
            continue;
        }
        let Some(binding) = uses
            .iter()
            .flatten()
            .flat_map(|used| used.bindings.iter())
            .find(|binding| {
                binding.name.text == first.name.text
                    && matches!(
                        binding.target,
                        item::Import::Module | item::Import::ModuleItem { .. }
                    )
            })
        else {
            continue;
        };
        let qualified = format!("{}{}", binding.qualified, &name[first.name.text.len()..]);
        if let Some(host::Entry::Type(definition)) = host.entry(&qualified) {
            types.insert(
                name,
                type_ref::Target::Host {
                    definition: definition.clone(),
                    owner: host.type_owner(&qualified).cloned(),
                },
            );
        }
    }
}

fn resolve_import_binding(
    name: crate::item::Name,
    qualified: ImmutableString,
    host: &host::Registry,
) -> Result<item::Binding, Error> {
    // A facade re-export binds the path it names: `iced::event::Event` is
    // `iced::Event`.
    let qualified = host.canonical_str(&std_reexport(&qualified, host));
    let target = match host.entry(&qualified) {
        Some(host::Entry::Module) => item::Import::Module,
        Some(host::Entry::ModuleItem(definitions)) => item::Import::ModuleItem {
            definitions: definitions.into(),
            owner: host.item_owner(&qualified).cloned(),
        },
        Some(host::Entry::Type(definition)) => item::Import::Type {
            definition: definition.clone(),
            owner: host.type_owner(&qualified).cloned(),
        },
        Some(host::Entry::Item(definitions)) => item::Import::Item {
            definitions: definitions.into(),
            owner: host.item_owner(&qualified).cloned(),
        },
        Some(host::Entry::Endpoint(contract)) => {
            let (_, owner) =
                host.operation_endpoint(&qualified)
                    .ok_or_else(|| Error::UnknownImport {
                        path: qualified.clone(),
                        reference: name.span,
                    })?;
            item::Import::Endpoint {
                contract: contract.clone(),
                owner: owner.clone(),
            }
        }
        // `use std::fmt;`, for hand-written `Debug` and `Display` impls,
        // and `use tokio::join;`, a macro Rawr expands.
        None if std_fmt_import(&qualified)
            || tokio_macro(&qualified, host).is_some()
            || qualified.contains("::") && hasher_alias(&qualified).is_some() =>
        {
            item::Import::Module
        }
        None => {
            return Err(Error::UnknownImport {
                path: qualified,
                reference: name.span,
            });
        }
    };
    Ok(item::Binding {
        name,
        qualified,
        target,
    })
}

/// The facade path a std path names, where a facade re-exports that std
/// module as its own: `std::time::Instant` is `iced::time::Instant`, as
/// iced's `time` re-exports std's types.
pub(crate) fn std_reexport(qualified: &str, host: &host::Registry) -> ImmutableString {
    // tokio in a program that does not import it, as a `?play` snippet, is
    // rawr-iced's: its timers follow a recording's clock. Its `Instant` is
    // the facade's, which reads that clock too, and its `Duration` is std's.
    const REEXPORTS: [(&str, &str); 8] = [
        ("std::time", "iced::time"),
        ("std::path", "rawr_iced::path"),
        ("std::ffi", "rawr_iced::ffi"),
        ("std::io", "rawr_iced::io"),
        ("core::time", "iced::time"),
        ("tokio::time::Instant", "iced::time::Instant"),
        ("tokio::time::Duration", "iced::time::Duration"),
        ("tokio", "rawr_iced::tokio"),
    ];
    // `spawn_blocking` takes a Rawr closure, which the imported tokio's
    // cannot, so it stays rawr-iced's in a program that imports tokio.
    const BLOCKING: &str = "tokio::task::spawn_blocking";
    if host.entry(qualified).is_some() {
        return qualified.into();
    }
    for (std, facade) in REEXPORTS {
        if std.starts_with("tokio")
            && host.facade_fingerprint("tokio").is_some()
            && qualified != BLOCKING
        {
            continue;
        }
        if let Some(rest) = qualified.strip_prefix(std)
            && (rest.is_empty() || rest.starts_with("::"))
        {
            let reexported = format!("{facade}{rest}");
            if host.entry(&reexported).is_some() {
                return reexported.into();
            }
        }
    }
    qualified.into()
}

/// A `RefCell` at runtime, as a host type whose values are cells names it.
#[doc(hidden)]
pub use lower_nominal::cell::Cell;

/// A `RefCell` of `value`, for a host type whose values are cells, as
/// rawr-iced's tokio `Mutex` is.
#[doc(hidden)]
pub fn cell_new(value: crate::Dynamic) -> crate::Dynamic {
    lower_nominal::cell_new(value)
}

/// Whether the `RefCell` `value` is borrowed; `None` when it is no cell.
#[doc(hidden)]
#[must_use]
pub fn cell_held(value: &crate::Dynamic) -> Option<bool> {
    lower_nominal::cell_held(value)
}

/// A mutable borrow of the `RefCell` `value`, as `borrow_mut` makes one,
/// when nothing borrows it; `None` while something does.
#[doc(hidden)]
#[must_use]
pub fn cell_lock(value: &crate::Dynamic) -> Option<crate::Dynamic> {
    lower_nominal::cell_lock(value)
}

/// The macro `path` names when it is tokio's `join`, `try_join` or
/// `select` in a program without tokio, whose tokio is rawr-iced's
/// (`std_reexport`). Rawr expands them itself.
pub(crate) fn tokio_macro(path: &str, host: &host::Registry) -> Option<&'static str> {
    if let Some(name) = path.strip_prefix("tokio::")
        && let Some(name) = ["join", "try_join", "select"]
            .into_iter()
            .find(|macro_name| *macro_name == name)
        && host.facade_fingerprint("tokio").is_none()
        && host.entry("rawr_iced::tokio").is_some()
    {
        return Some(name);
    }
    // A program whose futures are the core's (D513.5): the runtime's own
    // `join!` expands with private items, so Rawr writes it over
    // `rawr::future`'s combinators, for tokio's and the futures crate's.
    let name = path
        .strip_prefix("tokio::")
        .or_else(|| path.strip_prefix("futures::"))
        .or_else(|| path.strip_prefix("futures::future::"))?;
    let name = ["join", "try_join", "select"]
        .into_iter()
        .find(|macro_name| *macro_name == name)?;
    core_futures(host).then_some(name)
}

/// Whether async code in this program gives the core's
/// `rawr::future::Future`: its facade is installed and no host's own type,
/// such as iced's `Task`, runs futures.
pub(crate) fn core_futures(host: &host::Registry) -> bool {
    #[cfg(feature = "std")]
    return host.type_(crate::future::facade::FUTURE).is_some()
        && host.type_("iced::Task").is_none();
    #[cfg(not(feature = "std"))]
    return false;
}

/// `std::fmt` (or `core::fmt`) and its `Debug`, `Display` and `Formatter`,
/// and `std::cmp` and its `Ordering`, `PartialOrd`, `Ord`, `min` and `max`: imports a
/// hand-written `Debug`, `Display` or `Ord` impl names, which Rawr resolves
/// by spelling, so the import binds nothing else.
pub(crate) fn std_fmt_import(path: &str) -> bool {
    let rest = path
        .strip_prefix("std::")
        .or_else(|| path.strip_prefix("core::"));
    matches!(
        rest,
        Some(
            "fmt"
                | "fmt::Debug"
                | "fmt::Display"
                | "fmt::Formatter"
                | "fmt::Write"
                | "cmp"
                | "cmp::Ordering"
                | "cmp::PartialOrd"
                | "cmp::Ord"
                | "cmp::min"
                | "cmp::max"
                | "cmp::Reverse"
                | "collections"
                | "collections::HashMap"
                | "collections::HashSet"
                | "collections::BTreeMap"
                | "collections::BTreeSet"
                | "hash"
                | "hash::Hash"
                | "convert"
                | "convert::TryFrom"
                | "convert::TryInto"
                | "num"
                | "str::FromStr"
                | "mem"
                | "mem::swap"
                | "mem::replace"
                | "mem::take"
                | "iter"
                | "iter::once"
                | "iter::empty"
                | "iter::FromIterator"
                | "time"
                | "time::Duration"
                | "time::Instant"
                | "marker"
                | "marker::PhantomData"
                | "cell"
                | "cell::RefCell"
                | "rc"
                | "rc::Rc"
                | "sync::Arc"
        )
    ) || rest.is_some_and(|rest| std_parse_error(rest).is_some() && rest.contains("::"))
        || rest.is_some_and(|rest| {
            matches!(rest, "f32::consts" | "f64::consts") || float_constant(path).is_some()
        })
        || rest
            .and_then(|rest| rest.strip_prefix("ops"))
            .is_some_and(|rest| {
                rest.is_empty()
                    || rest.strip_prefix("::").is_some_and(|name| {
                        OPERATOR_TRAITS
                            .iter()
                            .any(|(_, trait_name, _)| *trait_name == name)
                            || matches!(name, "Index" | "IndexMut" | "Range" | "RangeInclusive")
                    })
            })
}

/// The names `use std::collections::*;` and its kin bind: each name of
/// that std module that `std_fmt_import` knows. None for another module.
pub(crate) fn std_glob(module: &str) -> Vec<ImmutableString> {
    const NAMES: &[&str] = &[
        "HashMap",
        "HashSet",
        "BTreeMap",
        "BTreeSet",
        "Debug",
        "Display",
        "Formatter",
        "Write",
        "Ordering",
        "PartialOrd",
        "Ord",
        "min",
        "max",
        "Reverse",
        "Hash",
        "TryFrom",
        "TryInto",
        "FromStr",
        "swap",
        "replace",
        "take",
        "Duration",
        "Instant",
    ];
    NAMES
        .iter()
        .filter(|name| std_fmt_import(&format!("{module}::{name}")))
        .map(|name| ImmutableString::from(*name))
        .collect()
}

/// `std::ops::RangeInclusive` and `std::ops::Range` as a type path is
/// written: the name, or one of its `ops::`, `std::ops::` or `core::ops::`
/// spellings. A `Range` is an iterator in Rawr; a `RangeInclusive` of
/// numbers is its own type.
pub(crate) fn std_range(path: &str) -> Option<&'static str> {
    let name = path
        .strip_prefix("std::ops::")
        .or_else(|| path.strip_prefix("core::ops::"))
        .or_else(|| path.strip_prefix("ops::"))
        .unwrap_or(path);
    match name {
        "RangeInclusive" => Some("RangeInclusive"),
        "Range" => Some(crate::item::type_ref::ITER_TYPE),
        _ => None,
    }
}

/// `std::f64::consts::PI` and kin as a path is written (`f32::consts::TAU`,
/// `core::f64::consts::E`): its value, as an `f64`, and whether it is an
/// `f32`'s.
pub(crate) fn float_constant(path: &str) -> Option<(f64, bool)> {
    let rest = path
        .strip_prefix("std::")
        .or_else(|| path.strip_prefix("core::"))
        .unwrap_or(path);
    let (kind, name) = rest.split_once("::consts::")?;
    macro_rules! constants {
        ($($name:ident),*) => {
            match (kind, name) {
                $(("f64", stringify!($name)) => Some((std::f64::consts::$name, false)),)*
                $(("f32", stringify!($name)) => Some((f64::from(std::f32::consts::$name), true)),)*
                _ => None,
            }
        };
    }
    constants!(
        PI,
        TAU,
        E,
        SQRT_2,
        FRAC_1_SQRT_2,
        LN_2,
        LN_10,
        LOG2_E,
        LOG10_E,
        LOG2_10,
        LOG10_2,
        FRAC_PI_2,
        FRAC_PI_3,
        FRAC_PI_4,
        FRAC_PI_6,
        FRAC_PI_8,
        FRAC_1_PI,
        FRAC_2_PI,
        FRAC_2_SQRT_PI
    )
}

/// `std::cmp::Reverse` as a type path is written: `Reverse` (imported, or
/// Rawr's when the crate has no type of that name), `cmp::Reverse`,
/// `std::cmp::Reverse`, `core::cmp::Reverse`.
pub(crate) fn std_reverse(path: &str) -> bool {
    matches!(
        path,
        "Reverse" | "cmp::Reverse" | "std::cmp::Reverse" | "core::cmp::Reverse"
    )
}

/// `std::collections::HashMap` and its kin as a type path is written:
/// `HashMap` (imported, or Rawr's when the crate has no type of that
/// name), `collections::HashMap`, `std::collections::HashMap`; the
/// collection's name.
pub(crate) fn std_collection(path: &str) -> Option<&'static str> {
    if let Some(collection) = hasher_alias(path) {
        return Some(collection);
    }
    let name = path
        .strip_prefix("std::collections::")
        .or_else(|| path.strip_prefix("collections::"))
        .unwrap_or(path);
    ["HashMap", "HashSet", "BTreeMap", "BTreeSet"]
        .into_iter()
        .find(|collection| *collection == name)
}

/// A crate's alias of std's `HashMap` or `HashSet` with its own hasher:
/// `rustc_hash::FxHashMap<K, V>` is `HashMap<K, V, FxBuildHasher>`. Rawr
/// reads it as its own map, whose hasher it does not expose; `cargo
/// build` compiles the crate's. The bare name, as `use` binds it, or the
/// full path; the std collection's name.
pub(crate) fn hasher_alias(path: &str) -> Option<&'static str> {
    const ALIASES: [(&str, &str); 2] = [("FxHashMap", "HashMap"), ("FxHashSet", "HashSet")];
    let name = ["rustc_hash::", "fxhash::"]
        .into_iter()
        .find_map(|krate| path.strip_prefix(krate))
        .unwrap_or(path);
    ALIASES
        .into_iter()
        .find_map(|(alias, collection)| (alias == name).then_some(collection))
}

/// `std::cmp::Ordering` as a type path is written: `Ordering` (imported,
/// or Rawr's when the crate has no type of that name), `cmp::Ordering`,
/// `std::cmp::Ordering`, `core::cmp::Ordering`.
/// `std::num::ParseIntError` and the other errors of `str::parse` as a
/// type path is written: bare (imported, or Rawr's when the crate has no
/// type of that name) or by its module; the error's name.
pub(crate) fn std_parse_error(path: &str) -> Option<&'static str> {
    let name = path
        .strip_prefix("std::")
        .or_else(|| path.strip_prefix("core::"))
        .unwrap_or(path);
    let (module, name) = name.rsplit_once("::").unwrap_or(("", name));
    let (expected, error) = match name {
        "ParseIntError" => ("num", "ParseIntError"),
        "ParseFloatError" => ("num", "ParseFloatError"),
        "ParseBoolError" => ("str", "ParseBoolError"),
        "ParseCharError" => ("char", "ParseCharError"),
        "Infallible" => ("convert", "Infallible"),
        "FromUtf8Error" => ("string", "FromUtf8Error"),
        "TryFromIntError" => ("num", "TryFromIntError"),
        _ => return None,
    };
    (module.is_empty() || module == expected).then_some(error)
}

/// `std::time::Duration` and `Instant` as a type path is written: bare
/// (imported, or Rawr's when the crate has no type of that name), or by
/// `time::`, `std::time::` or `core::time::`; the type's name.
pub(crate) fn std_time(path: &str) -> Option<&'static str> {
    let name = path
        .strip_prefix("std::time::")
        .or_else(|| path.strip_prefix("core::time::"))
        .or_else(|| path.strip_prefix("time::"))
        .unwrap_or(path);
    ["Duration", "Instant"]
        .into_iter()
        .find(|time| *time == name)
}

/// `std::cell::RefCell` as a path is written.
pub(crate) fn std_cell(path: &str) -> bool {
    matches!(
        path,
        "RefCell" | "cell::RefCell" | "std::cell::RefCell" | "core::cell::RefCell"
    )
}

/// Whether the `cfg(..)` text of a Cargo `[target.'cfg(..)'.dependencies]`
/// key holds on the target Rawr runs on: `None` when it is no `cfg`.
pub(crate) fn cfg_text_holds(text: &str) -> Option<bool> {
    fn predicate(text: &mut &str) -> Option<bool> {
        *text = text.trim_start();
        let end = text
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(text.len());
        let (name, rest) = text.split_at(end);
        *text = rest.trim_start();
        if let Some(rest) = text.strip_prefix('(') {
            *text = rest;
            let mut values = Vec::new();
            loop {
                *text = text.trim_start();
                if let Some(rest) = text.strip_prefix(')') {
                    *text = rest;
                    break;
                }
                values.push(predicate(text)?);
                *text = text.trim_start();
                if let Some(rest) = text.strip_prefix(',') {
                    *text = rest;
                }
            }
            return match name {
                "all" => Some(values.iter().all(|value| *value)),
                "any" => Some(values.iter().any(|value| *value)),
                "not" if values.len() == 1 => Some(!values[0]),
                _ => None,
            };
        }
        if let Some(rest) = text.strip_prefix('=') {
            let rest = rest.trim_start().strip_prefix('"')?;
            let close = rest.find('"')?;
            *text = &rest[close + 1..];
            return Some(lower_nominal::cfg_value(name, &rest[..close]));
        }
        Some(lower_nominal::cfg_flag(name))
    }
    let mut inner = text.trim().strip_prefix("cfg(")?.strip_suffix(')')?;
    let holds = predicate(&mut inner)?;
    inner.trim().is_empty().then_some(holds)
}

/// `std::rc::Rc` or `std::sync::Arc` as a path is written: the pointer's
/// name.
pub(crate) fn std_pointer(path: &str) -> Option<&'static str> {
    match path {
        "Rc" | "rc::Rc" | "std::rc::Rc" | "alloc::rc::Rc" => Some("Rc"),
        "Arc" | "sync::Arc" | "std::sync::Arc" | "alloc::sync::Arc" => Some("Arc"),
        _ => None,
    }
}

/// `std::marker::PhantomData` as a path is written.
pub(crate) fn std_phantom(path: &str) -> bool {
    matches!(
        path,
        "PhantomData"
            | "marker::PhantomData"
            | "std::marker::PhantomData"
            | "core::marker::PhantomData"
    )
}

pub(crate) fn std_ordering(path: &str) -> bool {
    matches!(
        path,
        "Ordering" | "cmp::Ordering" | "std::cmp::Ordering" | "core::cmp::Ordering"
    )
}

/// A path that starts with `::` names an outside crate, as in Rust (E0433):
/// `first` is a name of this program instead.
pub(crate) fn not_a_crate(first: &str, reference: Span) -> Error {
    Error::InvalidImport {
        path: format!("::{first}").into(),
        reason: format!(
            "a path that starts with `::` names an outside crate, and `{first}` is not one"
        )
        .into(),
        reference,
        declaration: None,
    }
}

fn parsed_path(path: &crate::item::type_ref::Path) -> ImmutableString {
    path.segments
        .iter()
        .map(|segment| segment.name.text.as_str())
        .collect::<Vec<_>>()
        .join("::")
        .into()
}

fn prefix_span(tree: &crate::item::UseTree) -> Span {
    match tree {
        crate::item::UseTree::Group { prefix, .. } | crate::item::UseTree::Glob { prefix, .. } => {
            prefix.span
        }
        crate::item::UseTree::Path(path, _) => path.span,
    }
}

fn resolve_item(
    id: crate::item::Id,
    source: program::source::Id,
    key: Option<crate::item::Key>,
    resolved_use: Option<item::Use>,
    parsed: crate::item::Parsed,
    types: &BTreeMap<ImmutableString, type_ref::Target>,
    host: &host::Registry,
    #[cfg(feature = "rust-import")] native: Option<native::Site<'_>>,
) -> Result<item::Shell, Error> {
    let span = parsed.span();
    let facades = facade_traits(&parsed, types, host);
    let kind = match parsed {
        crate::item::Parsed::TypeAlias(alias) => {
            return Err(Error::UnsupportedNominalSyntax {
                construct: "a type alias read as an item; its target is resolved with the \
                            module's types"
                    .into(),
                source: alias.span,
            });
        }
        crate::item::Parsed::Module(module) => {
            return Err(Error::UnsupportedNominalSyntax {
                construct: "file modules require project/package compilation".into(),
                source: module.span,
            });
        }
        crate::item::Parsed::Use(_) => item::Kind::Use(
            resolved_use.expect("predeclared use items retain their resolved bindings"),
        ),
        crate::item::Parsed::Struct(parsed) => {
            let (definition, key) = declared_type(&parsed.name, types);
            let (scope, generics) = generic_scope(
                parsed.generics,
                types,
                host,
                #[cfg(feature = "rust-import")]
                native,
            )?;
            let types = scope.as_ref().unwrap_or(types);
            let mut fields = Vec::with_capacity(parsed.fields.len());
            let mut names = BTreeMap::new();

            for field in parsed.fields.into_vec() {
                if let Some(previous_declaration) =
                    names.insert(field.name.text.clone(), field.name.span)
                {
                    return Err(Error::DuplicateMember {
                        owner: parsed.name.text.clone(),
                        member: field.name.text.clone(),
                        declaration: field.name.span,
                        previous_declaration,
                    });
                }
                let shared_str = matches!(
                    &field.ty,
                    crate::item::type_ref::Type::Reference { mutable: false, target, .. }
                        if matches!(&**target, crate::item::type_ref::Type::Path(path)
                            if path.segments.len() == 1 && path.segments[0].name.text == "str")
                );
                fields.push(item::Field {
                    shared_str,
                    name: field.name,
                    ty: type_ref::resolve(
                        field.ty,
                        types,
                        host,
                        #[cfg(feature = "rust-import")]
                        native,
                    )?,
                    span: field.span,
                });
            }

            item::Kind::Struct(item::Struct {
                header: parsed.header,
                definition,
                key,
                name: parsed.name,
                generics,
                fields: fields.into_boxed_slice(),
                form: parsed.form,
            })
        }
        crate::item::Parsed::Enum(parsed) => {
            let (definition, key) = declared_type(&parsed.name, types);
            let (scope, generics) = generic_scope(
                parsed.generics,
                types,
                host,
                #[cfg(feature = "rust-import")]
                native,
            )?;
            let types = scope.as_ref().unwrap_or(types);
            let mut variants = Vec::with_capacity(parsed.variants.len());
            let mut names = BTreeMap::new();

            for variant in parsed.variants.into_vec() {
                if let Some(previous_declaration) =
                    names.insert(variant.name.text.clone(), variant.name.span)
                {
                    return Err(Error::DuplicateMember {
                        owner: parsed.name.text.clone(),
                        member: variant.name.text.clone(),
                        declaration: variant.name.span,
                        previous_declaration,
                    });
                }
                let mut payload = Vec::with_capacity(variant.payload.len());

                for ty in variant.payload.into_vec() {
                    payload.push(type_ref::resolve(
                        ty,
                        types,
                        host,
                        #[cfg(feature = "rust-import")]
                        native,
                    )?);
                }

                variants.push(item::Variant {
                    name: variant.name,
                    payload: payload.into_boxed_slice(),
                    fields: variant.fields,
                    default: variant.default,
                    discriminant: variant.discriminant,
                    span: variant.span,
                });
            }

            item::Kind::Enum(item::Enum {
                header: parsed.header,
                definition,
                key,
                name: parsed.name,
                generics,
                variants: variants.into_boxed_slice(),
            })
        }
        crate::item::Parsed::Impl(parsed) => {
            let (scope, generics) = generic_scope(
                parsed.generics,
                types,
                host,
                #[cfg(feature = "rust-import")]
                native,
            )?;
            let types = scope.as_ref().unwrap_or(types);
            let trait_ = parsed
                .trait_
                .map(|path| {
                    resolve_trait(
                        path,
                        types,
                        host,
                        #[cfg(feature = "rust-import")]
                        native,
                    )
                })
                .transpose()?;
            let owner = type_ref::resolve(
                parsed.owner,
                types,
                host,
                #[cfg(feature = "rust-import")]
                native,
            )?;
            let mut functions = Vec::with_capacity(parsed.functions.len());

            for function in parsed.functions.into_vec() {
                functions.push(resolve_function(
                    function,
                    types,
                    host,
                    true,
                    #[cfg(feature = "rust-import")]
                    native,
                )?);
            }
            let mut associated = Vec::with_capacity(parsed.associated.len());
            for (name, ty) in parsed.associated.into_vec() {
                if let Some((previous, _)) = associated
                    .iter()
                    .find(|(declared, _): &&(crate::item::Name, _)| declared.text == name.text)
                {
                    return Err(Error::DuplicateMember {
                        owner: type_name(&owner),
                        member: name.text.clone(),
                        declaration: name.span,
                        previous_declaration: previous.span,
                    });
                }
                let ty = type_ref::resolve(
                    ty,
                    types,
                    host,
                    #[cfg(feature = "rust-import")]
                    native,
                )?;
                associated.push((name, ty));
            }
            if trait_.is_none()
                && let Some((name, _)) = associated.first()
            {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: "associated types in inherent impls (unstable in Rust)".into(),
                    source: name.span,
                });
            }

            item::Kind::Impl(item::Impl {
                header: parsed.header,
                generics,
                trait_,
                owner,
                functions: functions.into_boxed_slice(),
                associated: associated.into_boxed_slice(),
            })
        }
        crate::item::Parsed::Trait(parsed) => {
            // `Self` in a trait's signatures is the implementing type: a
            // generic parameter of the trait, beside its own (`T` of
            // `trait Convert<T>`).
            let (trait_scope, trait_generics) = generic_scope(
                parsed.generics,
                types,
                host,
                #[cfg(feature = "rust-import")]
                native,
            )?;
            let mut scope = trait_scope.unwrap_or_else(|| types.clone());
            scope.insert("Self".into(), type_ref::Target::Parameter);
            let mut functions = Vec::with_capacity(parsed.functions.len());
            let mut names = BTreeMap::new();
            for function in parsed.functions.into_vec() {
                if let Some(previous_declaration) =
                    names.insert(function.name.text.clone(), function.name.span)
                {
                    return Err(Error::DuplicateMember {
                        owner: parsed.name.text.clone(),
                        member: function.name.text.clone(),
                        declaration: function.name.span,
                        previous_declaration,
                    });
                }
                functions.push(resolve_function(
                    function,
                    &scope,
                    host,
                    true,
                    #[cfg(feature = "rust-import")]
                    native,
                )?);
            }
            // `trait Both<T>: Convert<T>`, `trait Ord2: PartialOrd2<Self>`:
            // a supertrait's arguments name the trait's parameters and
            // `Self`.
            let (supertrait_bounds, supertrait_names, written) =
                resolve_bounds_with_arguments(&parsed.supertraits)?;
            let supertrait_arguments = written
                .into_iter()
                .map(|(name, types)| {
                    types
                        .into_iter()
                        .map(|ty| {
                            type_ref::resolve(
                                ty,
                                &scope,
                                host,
                                #[cfg(feature = "rust-import")]
                                native,
                            )
                        })
                        .collect::<Result<Box<[_]>, _>>()
                        .map(|types| (name.text, types))
                })
                .collect::<Result<Box<[_]>, _>>()?;
            // `type Item: Into<String>;`: a generic trait's arguments, as a
            // supertrait's are.
            let mut associated_bounds = Vec::new();
            let mut associated_bound_arguments = Vec::new();
            for bounds in parsed.associated_bounds.iter() {
                let (capabilities, names, written) = resolve_bounds_with_arguments(bounds)?;
                associated_bounds.push((capabilities, names));
                associated_bound_arguments.push(
                    written
                        .into_iter()
                        .map(|(name, types)| {
                            types
                                .into_iter()
                                .map(|ty| {
                                    type_ref::resolve(
                                        ty,
                                        &scope,
                                        host,
                                        #[cfg(feature = "rust-import")]
                                        native,
                                    )
                                })
                                .collect::<Result<Box<[_]>, _>>()
                                .map(|types| (name.text, types))
                        })
                        .collect::<Result<Box<[_]>, _>>()?,
                );
            }
            item::Kind::Trait(item::Trait {
                header: parsed.header,
                name: parsed.name,
                functions: functions.into_boxed_slice(),
                provided: parsed.provided,
                associated: parsed.associated,
                associated_bounds: associated_bounds.into_boxed_slice(),
                associated_bound_arguments: associated_bound_arguments.into_boxed_slice(),
                supertraits: (supertrait_bounds, supertrait_names),
                supertrait_arguments,
                generics: trait_generics,
                declaration: None,
            })
        }
        crate::item::Parsed::Function(parsed) => item::Kind::Function(resolve_function(
            parsed,
            types,
            host,
            false,
            #[cfg(feature = "rust-import")]
            native,
        )?),
    };

    Ok(item::Shell {
        id,
        source,
        key,
        span,
        kind,
        facades,
    })
}

/// The declarations of the facade traits an impl's header or a type's
/// bounds name through `types`: `use iced::widget::Component;` or
/// `component::Component`, never a bare name the scope lacks (rustc's
/// E0405).
fn facade_traits(
    parsed: &crate::item::Parsed,
    types: &BTreeMap<ImmutableString, type_ref::Target>,
    host: &host::Registry,
) -> Box<[ImmutableString]> {
    let (generics, trait_) = match parsed {
        crate::item::Parsed::Struct(own) => (&own.generics, None),
        crate::item::Parsed::Enum(own) => (&own.generics, None),
        crate::item::Parsed::Impl(own) => (&own.generics, own.trait_.as_ref()),
        _ => return Box::default(),
    };
    generics
        .iter()
        .flat_map(|generic| generic.bounds.iter())
        .chain(trait_)
        .filter_map(
            |path| match type_ref::host_trait(path, types, host)?.shape() {
                host::type_def::Shape::Trait(declaration) => Some(declaration.clone()),
                _ => None,
            },
        )
        .collect()
}

fn resolve_function(
    mut parsed: crate::item::Function,
    types: &BTreeMap<ImmutableString, type_ref::Target>,
    host: &host::Registry,
    inside_impl: bool,
    #[cfg(feature = "rust-import")] native: Option<native::Site<'_>>,
) -> Result<item::Function, Error> {
    if !inside_impl && let Some(receiver) = &parsed.receiver {
        let reference = receiver.span();
        return Err(Error::SelfOutsideImpl { reference });
    }
    erase_trait_generics(&mut parsed, types, host);
    let (scope, generics) = generic_scope(
        parsed.generics,
        types,
        host,
        #[cfg(feature = "rust-import")]
        native,
    )?;
    let types = scope.as_ref().unwrap_or(types);

    let mut parameters = Vec::with_capacity(parsed.parameters.len());
    let mut parameter_names = BTreeMap::new();

    for parameter in parsed.parameters.into_vec() {
        if let Some(previous_declaration) =
            parameter_names.insert(parameter.name.text.clone(), parameter.name.span)
        {
            return Err(Error::DuplicateMember {
                owner: parsed.name.text.clone(),
                member: parameter.name.text.clone(),
                declaration: parameter.name.span,
                previous_declaration,
            });
        }
        parameters.push(item::Parameter {
            name: parameter.name,
            ty: type_ref::resolve(
                parameter.ty,
                types,
                host,
                #[cfg(feature = "rust-import")]
                native,
            )?,
            span: parameter.span,
            pattern: parameter.pattern,
        });
    }

    let output = parsed
        .output
        .map(|output| {
            type_ref::resolve(
                output,
                types,
                host,
                #[cfg(feature = "rust-import")]
                native,
            )
        })
        .transpose()?;

    let mut generics = generics;
    for generic in &mut generics {
        generic.host = parameters
            .iter()
            .map(|parameter| &parameter.ty)
            .chain(output.as_ref())
            .any(|ty| names_in_host_type(ty, &generic.name.text, false));
    }

    Ok(item::Function {
        header: parsed.header,
        name: parsed.name,
        generics,
        receiver: parsed.receiver,
        parameters: parameters.into_boxed_slice(),
        output,
        body: item::Body {
            span: parsed.body_span,
            lowering: item::Lowering::Unavailable,
            syntax: parsed.body_syntax,
            constant: parsed.constant,
            asynchronous: parsed.asynchronous,
            text: None,
            elide: None,
        },
    })
}

/// Whether `ty` names the generic parameter `name` as a type argument of a
/// host type, at any depth.
fn names_in_host_type(ty: &type_ref::Type, name: &str, inside_host: bool) -> bool {
    match ty {
        type_ref::Type::Unit(_) => false,
        type_ref::Type::Tuple { members, .. } => members
            .iter()
            .any(|member| names_in_host_type(member, name, inside_host)),
        type_ref::Type::Reference { target, .. } => names_in_host_type(target, name, inside_host),
        type_ref::Type::Path(path) => {
            if inside_host
                && matches!(path.target, type_ref::Target::Parameter)
                && path.name.as_str() == name
            {
                return true;
            }
            let host = inside_host || matches!(path.target, type_ref::Target::Host { .. });
            path.arguments.iter().any(|argument| match argument {
                type_ref::Argument::Type(ty) => names_in_host_type(ty, name, host),
                type_ref::Argument::Lifetime(_) => false,
            })
        }
    }
}

/// A generic parameter bounded by a facade trait (`W: Widget<Message>`, or
/// a parameter's `impl Widget<Message>` lifted to one) is the trait's `impl`
/// type, which is the type every implementor converts into.
fn erase_trait_generics(
    function: &mut crate::item::Function,
    types: &BTreeMap<ImmutableString, type_ref::Target>,
    host: &host::Registry,
) {
    use crate::item::type_ref::{Argument, Path, Segment, Type, IMPL_TYPE};
    fn substitute(ty: &mut Type, name: &str, erased: &Type) {
        match ty {
            Type::Path(path)
                if matches!(&path.segments[..], [segment]
                    if segment.name.text == name && segment.arguments.is_empty()) =>
            {
                *ty = erased.clone();
            }
            Type::Path(path) => {
                for segment in path.segments.iter_mut() {
                    for argument in segment.arguments.iter_mut() {
                        if let Argument::Type(ty) = argument {
                            substitute(ty, name, erased);
                        }
                    }
                }
            }
            Type::Reference { target, .. } => substitute(target, name, erased),
            Type::Tuple { members, .. } => {
                for member in members.iter_mut() {
                    substitute(member, name, erased);
                }
            }
            Type::Unit(_) => {}
        }
    }
    let (erased, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut function.generics)
        .into_vec()
        .into_iter()
        .partition(|generic| {
            generic
                .bounds
                .iter()
                .any(|bound| type_ref::host_trait(bound, types, host).is_some())
        });
    function.generics = kept.into_boxed_slice();
    for generic in erased {
        let span = generic.name.span;
        let erased = Type::Path(Path {
            segments: Box::new([Segment {
                name: crate::item::Name {
                    text: IMPL_TYPE.into(),
                    span,
                },
                arguments: generic
                    .bounds
                    .iter()
                    .map(|bound| Argument::Type(Type::Path(bound.clone())))
                    .collect(),
                span,
            }]),
            span,
            global: false,
        });
        for parameter in function.parameters.iter_mut() {
            substitute(&mut parameter.ty, &generic.name.text, &erased);
        }
        if let Some(output) = &mut function.output {
            substitute(output, &generic.name.text, &erased);
        }
    }
}

fn resolve_methods(
    items: &mut Vec<item::Shell>,
    definitions: &[type_def::Definition],
    host: &host::Registry,
) -> Result<(Vec<method::Definition>, Vec<method::Definition>), LocatedError> {
    declare_facade_traits(items, host)?;
    let items = items.as_mut_slice();
    elaborate_supertraits(items)?;
    resolve_projections(items)?;
    let traits = trait_table(items)?;
    fill_trait_defaults(items, &traits);
    check_generic_arity(items, &traits)?;
    check_trait_bounds(items, &traits)?;
    let definitions: BTreeMap<_, _> = definitions
        .iter()
        .map(|definition| (definition.id(), definition))
        .collect();
    let mut declarations =
        BTreeMap::<(type_def::Id, ImmutableString), (program::source::Id, Span)>::new();
    let mut methods = Vec::new();
    let mut builtin_methods = Vec::new();
    // `IndexMut`'s `Output` is its `Index` impl's: each owner's, by its
    // type's spelling.
    let index_outputs = items
        .iter()
        .filter_map(|shell| {
            let item::Kind::Impl(item) = &shell.kind else {
                return None;
            };
            let Some(item::TraitImpl::User { name, .. }) = &item.trait_ else {
                return None;
            };
            if name.text != "Index" {
                return None;
            }
            let (_, output) = item
                .associated
                .iter()
                .find(|(associated, _)| associated.text == "Output")?;
            Some((full_type_name(&item.owner), output.clone()))
        })
        .collect::<BTreeMap<_, _>>();

    for shell in items {
        let item::Kind::Impl(item) = &mut shell.kind else {
            continue;
        };
        if let Some(item::TraitImpl::User { name, arguments }) = &item.trait_ {
            let Some((trait_source, declared)) = traits.get(&name.text) else {
                return Err(LocatedError::primary(
                    shell.source,
                    Error::UnknownType {
                        name: name.text.clone(),
                        reference: name.span,
                    },
                ));
            };
            // `impl Shape for i64`: a crate trait for a builtin type, as
            // Rust's orphan rule allows; a std trait is E0117.
            // A std trait is allowed when one of its type arguments is a
            // crate type (`impl From<Celsius> for f64`), as Rust's rule has
            // it.
            let local_argument = arguments.iter().any(|argument| {
                matches!(
                    argument,
                    type_ref::Type::Path(path)
                        if matches!(path.target, type_ref::Target::Script { .. })
                )
            });
            // A facade type (`impl From<Shade> for iced::Color`) owns impls
            // under the same rule.
            let outside = builtin_owner(&item.owner)
                .map(|(code, name)| (Some(code), name))
                .or_else(|| foreign_owner(&item.owner).map(|path| (None, path)));
            let builtin = outside
                .map(|(code, owner_name)| {
                    if declared.name.span == Span::NONE && !local_argument {
                        return Err(Error::InvalidImplOwner {
                            owner: format!(
                                "{owner_name}: only traits defined in the current crate can be \
                                 implemented for types defined outside of the crate (E0117)"
                            )
                            .into(),
                            reference: type_span(&item.owner),
                        });
                    }
                    Ok(match code {
                        Some(code) => type_def::Definition::builtin(
                            code,
                            &owner_name,
                            shell.id,
                            type_span(&item.owner),
                        ),
                        None => type_def::Definition::foreign(
                            &owner_name,
                            shell.id,
                            type_span(&item.owner),
                        ),
                    })
                })
                .transpose()
                .map_err(|error| LocatedError::primary(shell.source, error))?;
            let (owner_id, owner_name, owner) = match &builtin {
                Some(owner) => (owner.id(), ImmutableString::from(owner.name()), owner),
                None => {
                    let (owner_id, owner_name) = impl_owner(&item.owner)
                        .map_err(|error| LocatedError::primary(shell.source, error))?;
                    let owner = definitions
                        .get(&owner_id)
                        .copied()
                        .expect("resolved impl owners retain their generation-local definition");
                    (owner_id, owner_name, owner)
                }
            };
            // `impl Convert<i64> for C`: one argument per trait parameter
            // (E0107).
            if arguments.len() != declared.generics.len() {
                return Err(LocatedError::primary(
                    shell.source,
                    Error::InvalidTypeArguments {
                        ty: name.text.clone(),
                        expected: declared.generics.len(),
                        actual: arguments.len(),
                        reference: name.span,
                    },
                ));
            }
            let parameters = declared
                .generics
                .iter()
                .map(|generic| generic.name.text.clone())
                .zip(arguments.iter().cloned())
                .collect::<Vec<_>>();
            let mut associated = item.associated.to_vec();
            if name.text == "IndexMut"
                && declared.name.span == Span::NONE
                && let Some(output) = index_outputs.get(&full_type_name(&item.owner))
            {
                associated.push((
                    crate::item::Name {
                        text: "Output".into(),
                        span: Span::NONE,
                    },
                    output.clone(),
                ));
            }
            let implemented = trait_impl_functions(
                declared,
                &owner_name,
                &item.owner,
                &item.functions,
                &associated,
                &parameters,
                name.span,
            )
            .map_err(|error| LocatedError::primary(shell.source, error))?;
            // Each instance of a generic trait is its own trait:
            // `Convert<i64>::convert` beside `Convert<String>::convert`.
            let instance: ImmutableString = if arguments.is_empty() {
                name.text.clone()
            } else {
                format!(
                    "{}<{}>",
                    name.text,
                    arguments
                        .iter()
                        .map(full_type_name)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
                .into()
            };
            // A std `fmt` takes its formatter as `&mut`, which the trait's
            // own signature has already admitted.
            let formatter = declared.name.span == Span::NONE
                && matches!(name.text.as_str(), "Debug" | "Display");
            for function in &mut item.functions {
                if !formatter {
                    validate_method_signature(owner.name(), function, false, true)
                        .map_err(|error| LocatedError::primary(shell.source, error))?;
                }
            }
            for (function, from_trait) in implemented {
                let source = if from_trait {
                    *trait_source
                } else {
                    shell.source
                };
                let member: ImmutableString = format!("{instance}::{}", function.name.text).into();
                if let Some((previous_source, previous_declaration)) =
                    declarations.insert((owner_id, member.clone()), (shell.source, name.span))
                {
                    return Err(LocatedError::related(
                        shell.source,
                        previous_source,
                        Error::DuplicateMethod {
                            owner: owner_name.clone(),
                            method: member,
                            declaration: name.span,
                            previous_declaration,
                        },
                    ));
                }
                // Impls for builtin types number apart, so the program's
                // own methods keep their dense ids.
                let id = if builtin.is_some() {
                    method::Id::from_index(BUILTIN_METHOD_IDS + builtin_methods.len())
                } else {
                    method::Id::from_index(methods.len())
                };
                let kind = if function.receiver.is_some() {
                    crate::item::Kind::Method
                } else {
                    crate::item::Kind::AssociatedFunction
                };
                let key = crate::item::Key::associated(owner.key(), member, kind);
                let mut definition = method::Definition::new(id, key, owner, &function, source)
                    .in_generic_impl(&item.generics, owner_arguments(&item.owner))
                    .with_trait(name.text.clone())
                    .with_trait_arguments(arguments.clone());
                if from_trait {
                    definition = definition.into_trait_default();
                    if let Some(text) = &declared.declaration {
                        definition = definition.with_body_text(text.clone());
                    }
                }
                if builtin.is_some() {
                    definition = definition.for_builtin(item.owner.clone());
                }
                // The prelude's `From<T>`: the conversion `.into()` and `?`
                // choose by `T`.
                if name.text == "From"
                    && declared.name.span == Span::NONE
                    && let [source] = &arguments[..]
                {
                    definition = definition.with_conversion(source.clone());
                }
                if let Some(own) = item
                    .functions
                    .iter_mut()
                    .find(|own| own.name.text == function.name.text)
                {
                    own.body.lowering = item::Lowering::NominalMethod(id);
                }
                if builtin.is_some() {
                    builtin_methods.push(definition);
                } else {
                    methods.push(definition);
                }
            }
            continue;
        }
        let (owner_id, owner_name) =
            impl_owner(&item.owner).map_err(|error| LocatedError::primary(shell.source, error))?;
        let owner = definitions
            .get(&owner_id)
            .copied()
            .expect("resolved impl owners retain their generation-local definition");

        let widget_protocol = item
            .header
            .attributes
            .iter()
            .any(crate::item::Attribute::is_widget);
        for function in &mut item.functions {
            validate_method_signature(owner.name(), function, widget_protocol, true)
                .map_err(|error| LocatedError::primary(shell.source, error))?;
            let name = function.name.text.clone();
            let duplicate_key = (owner_id, name.clone());
            if let Some((previous_source, previous_declaration)) =
                declarations.insert(duplicate_key, (shell.source, function.name.span))
            {
                return Err(LocatedError::related(
                    shell.source,
                    previous_source,
                    Error::DuplicateMethod {
                        owner: owner_name.clone(),
                        method: function.name.text.clone(),
                        declaration: function.name.span,
                        previous_declaration,
                    },
                ));
            }

            let id = method::Id::from_index(methods.len());
            let kind = if function.receiver.is_some() {
                crate::item::Kind::Method
            } else {
                crate::item::Kind::AssociatedFunction
            };
            let key = crate::item::Key::associated(owner.key(), name, kind);
            let definition = method::Definition::new(id, key, owner, function, shell.source)
                .in_generic_impl(&item.generics, owner_arguments(&item.owner));
            function.body.lowering = item::Lowering::NominalMethod(id);
            methods.push(definition);
        }
    }

    Ok((methods, builtin_methods))
}

/// The first id of the methods of impls for builtin types.
const BUILTIN_METHOD_IDS: usize = 1 << 30;

/// A facade type as a trait impl owner: its path.
fn foreign_owner(ty: &type_ref::Type) -> Option<ImmutableString> {
    match ty {
        type_ref::Type::Path(path) => match &path.target {
            type_ref::Target::Host { definition, .. } => Some(definition.path().into()),
            _ => None,
        },
        _ => None,
    }
}

/// A builtin type as a trait impl owner: its witness code
/// and name. `Box<T>` is not one: it is `T` at runtime.
pub(crate) fn builtin_owner(ty: &type_ref::Type) -> Option<(i64, ImmutableString)> {
    let code = match ty {
        type_ref::Type::Unit(_) => return Some((-9, "()".into())),
        type_ref::Type::Path(path) if path.target == type_ref::Target::Builtin => {
            match path.name.as_str() {
                // Every integer is the runtime's one integer, and every
                // float its one float.
                "i8" | "i16" | "i32" | "i64" | "isize" | "u8" | "u16" | "u32" | "u64" | "usize" => {
                    -2
                }
                "f32" | "f64" => -3,
                "String" => -4,
                "bool" => -5,
                "char" => -6,
                "Option" => -7,
                "Vec" => -8,
                "Result" => -10,
                _ => return None,
            }
        }
        _ => return None,
    };
    let type_ref::Type::Path(path) = ty else {
        return None;
    };
    Some((code, path.name.clone()))
}

/// A generic trait's arguments with its defaults filled in, `Self` as
/// `self_ty` and an earlier parameter as its argument: `Add` is
/// `Add<Self>` of `trait Add<Rhs = Self>`. A missing argument
/// without a default stays missing, for the arity check to report.
fn with_trait_defaults(
    declared: &item::Trait,
    written: &[type_ref::Type],
    self_ty: &type_ref::Type,
) -> Box<[type_ref::Type]> {
    let mut arguments = written.to_vec();
    for generic in declared.generics.iter().skip(written.len()) {
        let Some(default) = &generic.default else {
            break;
        };
        let parameters = declared
            .generics
            .iter()
            .map(|generic| generic.name.text.clone())
            .zip(arguments.iter().cloned())
            .collect::<Vec<_>>();
        let filled = substitute_self(
            &substitute_parameters(default, &parameters),
            self_ty,
            &[],
            &declared.name.text,
        );
        arguments.push(filled);
    }
    arguments.into_boxed_slice()
}

/// Fill each generic trait's default arguments into the bounds that use
/// the trait (`T: Add` is `T: Add<T>`) and into its impls (`impl Add for
/// M` is `impl Add<M> for M`), as Rust reads them.
fn fill_trait_defaults(
    items: &mut [item::Shell],
    traits: &BTreeMap<ImmutableString, (program::source::Id, item::Trait)>,
) {
    if !traits.values().any(|(_, declared)| {
        declared
            .generics
            .iter()
            .any(|generic| generic.default.is_some())
    }) {
        return;
    }
    let fill = |generics: &mut [item::Generic]| {
        for generic in generics {
            let self_ty = type_ref::Type::Path(type_ref::Path {
                name: generic.name.text.clone(),
                target: type_ref::Target::Parameter,
                arguments: Box::default(),
                span: generic.name.span,
            });
            let mut arguments = generic.trait_arguments.to_vec();
            for name in generic.traits.iter() {
                let Some((_, declared)) = traits.get(&name.text) else {
                    continue;
                };
                if declared.generics.is_empty() {
                    continue;
                }
                if !arguments
                    .iter()
                    .any(|(trait_name, _)| *trait_name == name.text)
                {
                    arguments.push((name.text.clone(), Box::default()));
                }
                for (trait_name, written) in &mut arguments {
                    if *trait_name == name.text && written.len() < declared.generics.len() {
                        *written = with_trait_defaults(declared, written, &self_ty);
                    }
                }
            }
            arguments.retain(|(_, written)| !written.is_empty());
            generic.trait_arguments = arguments.into_boxed_slice();
        }
    };
    for shell in items.iter_mut() {
        match &mut shell.kind {
            item::Kind::Struct(item) => fill(&mut item.generics),
            item::Kind::Enum(item) => fill(&mut item.generics),
            item::Kind::Function(item) => fill(&mut item.generics),
            item::Kind::Impl(item) => {
                fill(&mut item.generics);
                for function in item.functions.iter_mut() {
                    fill(&mut function.generics);
                }
                if let Some(item::TraitImpl::User { name, arguments }) = &mut item.trait_
                    && let Some((_, declared)) = traits.get(&name.text)
                    && arguments.len() < declared.generics.len()
                {
                    *arguments = with_trait_defaults(declared, arguments, &item.owner);
                }
            }
            item::Kind::Trait(declared) => {
                for function in declared.functions.iter_mut() {
                    fill(&mut function.generics);
                }
            }
            item::Kind::Use(_) => {}
        }
    }
}

/// Supertraits as Rust elaborates them: each trait's `supertraits` becomes
/// its transitive closure, and every bound by a trait (`T: Named`, also
/// `type Output: Named;`) implies its supertraits' capabilities and traits,
/// so a generic body can use them. Unknown supertraits and cycles (E0391)
/// are errors.
fn elaborate_supertraits(items: &mut [item::Shell]) -> Result<(), LocatedError> {
    let declared: BTreeMap<ImmutableString, (program::source::Id, item::Trait)> = items
        .iter()
        .filter_map(|shell| match &shell.kind {
            item::Kind::Trait(declared) => {
                Some((declared.name.text.clone(), (shell.source, declared.clone())))
            }
            _ => None,
        })
        .collect();
    if declared.values().all(|(_, declared)| {
        declared.supertraits.1.is_empty() && declared.supertraits.0.iter().next().is_none()
    }) {
        return Ok(());
    }
    let union = |a: crate::capability::Set, b: crate::capability::Set| {
        crate::capability::Set::from_explicit(a.iter().chain(b.iter()))
    };
    // Each trait's closure, by depth-first search; a trait met again on
    // its own path is a cycle. A supertrait's arguments are in the trait's
    // own parameters and `Self`; a grandparent's are substituted through
    // its parent's (`trait C<T>: B<T>`, `trait B<U>: A<Vec<U>>` gives `C`
    // the supertrait `A<Vec<T>>`).
    fn close(
        name: &ImmutableString,
        declared: &BTreeMap<ImmutableString, (program::source::Id, item::Trait)>,
        path: &mut Vec<ImmutableString>,
        closed: &mut BTreeMap<ImmutableString, Supers>,
    ) -> Result<Supers, LocatedError> {
        if let Some(done) = closed.get(name) {
            return Ok(done.clone());
        }
        let Some((source, own)) = declared.get(name) else {
            // A std trait (`Display`) has no supertraits Rawr tracks.
            return Ok(Supers::default());
        };
        path.push(name.clone());
        let mut all_bounds = own.supertraits.0;
        let mut all: Vec<crate::item::Name> = Vec::new();
        let mut arguments: Vec<item::TraitArgument> = Vec::new();
        for parent in own.supertraits.1.iter() {
            if path.contains(&parent.text) {
                return Err(LocatedError::primary(
                    *source,
                    Error::InvalidMethodSignature {
                        owner: name.clone(),
                        method: parent.text.clone(),
                        reason: format!(
                            "cycle detected when computing the supertraits of `{name}` (E0391)"
                        )
                        .into(),
                        signature: parent.span,
                    },
                ));
            }
            if !declared.contains_key(&parent.text)
                && !BUILTIN_TRAITS.contains(&parent.text.as_str())
            {
                return Err(LocatedError::primary(
                    *source,
                    Error::UnknownType {
                        name: parent.text.clone(),
                        reference: parent.span,
                    },
                ));
            }
            let parent_closed = close(&parent.text, declared, path, closed)?;
            all_bounds = crate::capability::Set::from_explicit(
                all_bounds.iter().chain(parent_closed.0.iter()),
            );
            if !all.iter().any(|known| known.text == parent.text) {
                all.push(parent.clone());
            }
            // The parent's own arguments, its defaults filled in (`Self` of
            // `trait PartialOrd2<Rhs = Self>` is this trait's `Self`).
            let written: Vec<Box<[type_ref::Type]>> = own
                .supertrait_arguments
                .iter()
                .filter(|(trait_name, _)| *trait_name == parent.text)
                .map(|(_, types)| types.clone())
                .collect();
            let parent_trait = declared.get(&parent.text).map(|(_, parent)| parent);
            let written = match parent_trait {
                Some(parent_trait) if !parent_trait.generics.is_empty() => {
                    let self_ty = self_parameter(parent.span);
                    let lists = if written.is_empty() {
                        vec![Box::default()]
                    } else {
                        written
                    };
                    lists
                        .into_iter()
                        .map(|types| {
                            if types.len() < parent_trait.generics.len() {
                                with_trait_defaults(parent_trait, &types, &self_ty)
                            } else {
                                types
                            }
                        })
                        .filter(|types| !types.is_empty())
                        .collect()
                }
                _ => written,
            };
            for types in &written {
                push_arguments(&mut arguments, &parent.text, types.clone());
                let parameters = parent_trait
                    .map(|parent_trait| {
                        parent_trait
                            .generics
                            .iter()
                            .map(|generic| generic.name.text.clone())
                            .zip(types.iter().cloned())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                for (grandparent, inherited) in parent_closed.2.iter() {
                    let substituted = inherited
                        .iter()
                        .map(|ty| substitute_parameters(ty, &parameters))
                        .collect();
                    push_arguments(&mut arguments, grandparent, substituted);
                }
            }
            for name in parent_closed.1.iter() {
                if !all.iter().any(|known| known.text == name.text) {
                    all.push(name.clone());
                }
            }
            // A non-generic parent's generic supertraits name only `Self`.
            if written.is_empty() {
                for (grandparent, inherited) in parent_closed.2.iter() {
                    push_arguments(&mut arguments, grandparent, inherited.clone());
                }
            }
        }
        path.pop();
        let supers: Supers = (all_bounds, all.into_boxed_slice(), arguments);
        closed.insert(name.clone(), supers.clone());
        Ok(supers)
    }
    type Supers = (
        crate::capability::Set,
        Box<[crate::item::Name]>,
        Vec<item::TraitArgument>,
    );
    fn push_arguments(
        arguments: &mut Vec<item::TraitArgument>,
        trait_name: &ImmutableString,
        types: Box<[type_ref::Type]>,
    ) {
        let text = |types: &[type_ref::Type]| {
            types
                .iter()
                .map(full_type_name)
                .collect::<Vec<_>>()
                .join(", ")
        };
        if !arguments
            .iter()
            .any(|(known, known_types)| known == trait_name && text(known_types) == text(&types))
        {
            arguments.push((trait_name.clone(), types));
        }
    }
    let mut closed = BTreeMap::new();
    for name in declared.keys() {
        close(name, &declared, &mut Vec::new(), &mut closed)?;
    }
    // A bound `T: Both<i64>` implies `T: Convert<i64>`: the closure's
    // arguments with the trait's parameters replaced by the bound's and
    // `Self` by the bounded type.
    let elaborate = |bounded: Option<&crate::item::Name>,
                     bounds: &mut crate::capability::Set,
                     traits: &mut Box<[crate::item::Name]>,
                     trait_arguments: Option<&mut Box<[item::TraitArgument]>>| {
        let mut all = traits.to_vec();
        let mut implied = Vec::new();
        for name in traits.iter() {
            let Some((parent_bounds, parent_traits, parent_arguments)) = closed.get(&name.text)
            else {
                continue;
            };
            *bounds = union(*bounds, *parent_bounds);
            for parent in parent_traits.iter() {
                if !all.iter().any(|known| known.text == parent.text) {
                    all.push(parent.clone());
                }
            }
            let (Some(bounded), Some(written)) = (bounded, trait_arguments.as_deref()) else {
                continue;
            };
            if parent_arguments.is_empty() {
                continue;
            }
            let self_ty = type_ref::Type::Path(type_ref::Path {
                name: bounded.text.clone(),
                target: type_ref::Target::Parameter,
                arguments: Box::default(),
                span: bounded.span,
            });
            let own_parameters = declared
                .get(&name.text)
                .map(|(_, own)| {
                    own.generics
                        .iter()
                        .map(|generic| generic.name.text.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let mut lists: Vec<Box<[type_ref::Type]>> = written
                .iter()
                .filter(|(trait_name, _)| *trait_name == name.text)
                .map(|(_, types)| types.clone())
                .collect();
            if lists.is_empty() {
                lists.push(Box::default());
            }
            for list in lists {
                let mut parameters = own_parameters
                    .iter()
                    .cloned()
                    .zip(list.iter().cloned())
                    .collect::<Vec<_>>();
                parameters.push(("Self".into(), self_ty.clone()));
                for (parent, types) in parent_arguments {
                    implied.push((
                        parent.clone(),
                        types
                            .iter()
                            .map(|ty| substitute_parameters(ty, &parameters))
                            .collect::<Box<[_]>>(),
                    ));
                }
            }
        }
        *traits = all.into_boxed_slice();
        if let Some(trait_arguments) = trait_arguments {
            let mut arguments = trait_arguments.to_vec();
            for (parent, types) in implied {
                push_arguments(&mut arguments, &parent, types);
            }
            *trait_arguments = arguments.into_boxed_slice();
        }
    };
    let generics = |generics: &mut [item::Generic]| {
        for generic in generics {
            elaborate(
                Some(&generic.name.clone()),
                &mut generic.bounds,
                &mut generic.traits,
                Some(&mut generic.trait_arguments),
            );
        }
    };
    for shell in items.iter_mut() {
        match &mut shell.kind {
            item::Kind::Struct(item) => generics(&mut item.generics),
            item::Kind::Enum(item) => generics(&mut item.generics),
            item::Kind::Impl(item) => {
                generics(&mut item.generics);
                for function in item.functions.iter_mut() {
                    generics(&mut function.generics);
                }
            }
            item::Kind::Trait(declared) => {
                if let Some((bounds, traits, arguments)) = closed.get(&declared.name.text) {
                    declared.supertraits = (*bounds, traits.clone());
                    declared.supertrait_arguments = arguments.clone().into_boxed_slice();
                }
                for function in declared.functions.iter_mut() {
                    generics(&mut function.generics);
                }
                for (bounds, traits) in declared.associated_bounds.iter_mut() {
                    elaborate(None, bounds, traits, None);
                }
            }
            item::Kind::Function(function) => generics(&mut function.generics),
            item::Kind::Use(_) => {}
        }
    }
    Ok(())
}

/// The trait parameter `Self`.
fn self_parameter(span: Span) -> type_ref::Type {
    type_ref::Type::Path(type_ref::Path {
        name: "Self".into(),
        target: type_ref::Target::Parameter,
        arguments: Box::default(),
        span,
    })
}

/// The facade traits the crate's items name (iced's
/// `component::Component`), resolved from their declarations as the
/// crate's own traits are; a crate trait of the same name shadows one.
fn declare_facade_traits(
    items: &mut Vec<item::Shell>,
    host: &host::Registry,
) -> Result<(), LocatedError> {
    {
        let declared = items
            .iter()
            .filter_map(|shell| match &shell.kind {
                item::Kind::Trait(own) => Some(own.name.text.clone()),
                _ => None,
            })
            .collect::<std::collections::BTreeSet<_>>();
        let declarations = items
            .iter()
            .flat_map(|shell| shell.facades.iter().cloned())
            .collect::<std::collections::BTreeSet<_>>();
        for declaration in declarations {
            let Some(parsed) = crate::parser::parse_trait(&declaration) else {
                continue;
            };
            if declared.contains(&parsed.name.text) {
                continue;
            }
            let shell = resolve_item(
                crate::item::Id::from_index(items.len()),
                program::source::Id::ROOT,
                None,
                None,
                crate::item::Parsed::Trait(parsed),
                &BTreeMap::new(),
                host,
                #[cfg(feature = "rust-import")]
                None,
            )
            .map_err(|error| LocatedError::primary(program::source::Id::ROOT, error))?;
            let mut shell = shell;
            if let item::Kind::Trait(declared) = &mut shell.kind {
                declared.declaration = Some(declaration.clone());
            }
            items.push(shell);
        }
    }
    Ok(())
}

/// The crate's trait declarations by name, with their sources.
fn trait_table(
    items: &[item::Shell],
) -> Result<BTreeMap<ImmutableString, (program::source::Id, item::Trait)>, LocatedError> {
    let mut traits = BTreeMap::<ImmutableString, (program::source::Id, item::Trait)>::new();
    for shell in items {
        let item::Kind::Trait(declared) = &shell.kind else {
            continue;
        };
        if let Some((previous_source, previous)) =
            traits.insert(declared.name.text.clone(), (shell.source, declared.clone()))
        {
            return Err(LocatedError::related(
                shell.source,
                previous_source,
                Error::DuplicateItem {
                    name: declared.name.text.clone(),
                    declaration: declared.name.span,
                    previous_declaration: previous.name.span,
                },
            ));
        }
    }
    // The std traits implemented by hand, unless the crate declares its own.
    for name in BUILTIN_TRAITS {
        if let Some(declared) = builtin_trait(name) {
            traits
                .entry(name.into())
                .or_insert((program::source::Id::ROOT, declared));
        }
    }
    Ok(traits)
}

/// The std traits a crate may implement by hand: the derivable ones,
/// `Display` for `{}`, the prelude's `From<T>` and `Into<T>`, and
/// `PartialOrd` and `Ord`, which compare to a `std::cmp::Ordering`.
pub(crate) const BUILTIN_TRAITS: [&str; 39] = [
    "Clone",
    "Hash",
    "Copy",
    "PartialEq",
    "Eq",
    "Default",
    "Debug",
    "Display",
    "From",
    "Into",
    "PartialOrd",
    "Ord",
    "Iterator",
    "IntoFragment",
    "Add",
    "Sub",
    "Mul",
    "Div",
    "Rem",
    "Neg",
    "Not",
    "AddAssign",
    "SubAssign",
    "MulAssign",
    "DivAssign",
    "RemAssign",
    "BitAnd",
    "BitOr",
    "BitXor",
    "Shl",
    "Shr",
    "BitAndAssign",
    "BitOrAssign",
    "BitXorAssign",
    "ShlAssign",
    "ShrAssign",
    "Index",
    "IndexMut",
    "FromIterator",
];

/// `std::ops`' operator traits Rawr dispatches its operators through for a
/// crate type: the operator, the trait and its method.
pub(crate) const OPERATOR_TRAITS: [(&str, &str, &str); 22] = [
    ("+", "Add", "add"),
    ("-", "Sub", "sub"),
    ("*", "Mul", "mul"),
    ("/", "Div", "div"),
    ("%", "Rem", "rem"),
    ("neg", "Neg", "neg"),
    ("!", "Not", "not"),
    ("+=", "AddAssign", "add_assign"),
    ("-=", "SubAssign", "sub_assign"),
    ("*=", "MulAssign", "mul_assign"),
    ("/=", "DivAssign", "div_assign"),
    ("%=", "RemAssign", "rem_assign"),
    ("&", "BitAnd", "bitand"),
    ("|", "BitOr", "bitor"),
    ("^", "BitXor", "bitxor"),
    ("<<", "Shl", "shl"),
    (">>", "Shr", "shr"),
    ("&=", "BitAndAssign", "bitand_assign"),
    ("|=", "BitOrAssign", "bitor_assign"),
    ("^=", "BitXorAssign", "bitxor_assign"),
    ("<<=", "ShlAssign", "shl_assign"),
    (">>=", "ShrAssign", "shr_assign"),
];

/// A std trait named by its path, as `std::fmt::Display` or `fmt::Debug`.
fn std_trait(segments: &[crate::item::type_ref::Segment]) -> Option<crate::item::Name> {
    let [prefix @ .., last] = segments else {
        return None;
    };
    (!prefix.is_empty()
        && last.arguments.is_empty()
        && BUILTIN_TRAITS.contains(&last.name.text.as_str())
        && prefix.iter().all(|module| {
            module.arguments.is_empty()
                && matches!(
                    module.name.text.as_str(),
                    "std"
                        | "core"
                        | "fmt"
                        | "clone"
                        | "cmp"
                        | "default"
                        | "marker"
                        | "iter"
                        | "ops"
                        | "hash"
                )
        }))
    .then(|| last.name.clone())
}

/// The declaration of a std trait Rawr implements by hand. `Debug` and
/// `Display` write into a `fmt::Formatter`, which is Rawr's `String`, and
/// return `fmt::Result`, `Result<(), ()>`.
pub(crate) fn builtin_trait(name: &str) -> Option<item::Trait> {
    let path = |name: &str, target: type_ref::Target, arguments: Vec<type_ref::Argument>| {
        type_ref::Type::Path(type_ref::Path {
            name: name.into(),
            target,
            arguments: arguments.into_boxed_slice(),
            span: Span::NONE,
        })
    };
    let this = || path("Self", type_ref::Target::Parameter, Vec::new());
    let reference = |mutable: bool, target: type_ref::Type| type_ref::Type::Reference {
        mutable,
        lifetime: None,
        target: Box::new(target),
        span: Span::NONE,
    };
    let function = |name: &str,
                    receiver: bool,
                    parameters: Vec<(&str, type_ref::Type)>,
                    output: Option<type_ref::Type>| item::Function {
        header: crate::item::Header {
            attributes: Box::default(),
            visibility: None,
            span: Span::NONE,
        },
        name: crate::item::Name {
            text: name.into(),
            span: Span::NONE,
        },
        generics: Box::default(),
        receiver: receiver.then_some(crate::item::Receiver::Shared(Span::NONE)),
        parameters: parameters
            .into_iter()
            .map(|(name, ty)| item::Parameter {
                name: crate::item::Name {
                    text: name.into(),
                    span: Span::NONE,
                },
                ty,
                span: Span::NONE,
                pattern: None,
            })
            .collect(),
        output,
        body: item::Body {
            span: Span::NONE,
            lowering: item::Lowering::Unavailable,
            syntax: crate::item::BodySyntax::new(Vec::new()),
            constant: false,
            asynchronous: false,
            text: None,
            elide: None,
        },
    };
    let functions = match name {
        "Clone" => vec![function("clone", true, Vec::new(), Some(this()))],
        "PartialEq" => vec![function(
            "eq",
            true,
            vec![("other", reference(false, this()))],
            Some(path("bool", type_ref::Target::Builtin, Vec::new())),
        )],
        "Default" => vec![function("default", false, Vec::new(), Some(this()))],
        "Debug" | "Display" => vec![function(
            "fmt",
            true,
            vec![(
                "f",
                reference(true, path("String", type_ref::Target::Builtin, Vec::new())),
            )],
            Some(path(
                "Result",
                type_ref::Target::Builtin,
                vec![
                    type_ref::Argument::Type(type_ref::Type::Unit(Span::NONE)),
                    type_ref::Argument::Type(type_ref::Type::Unit(Span::NONE)),
                ],
            )),
        )],
        // `fn partial_cmp(&self, other: &Self) -> Option<Ordering>`, and
        // `Ord`'s `cmp` to an `Ordering`.
        "PartialOrd" | "Ord" => {
            let ordering = path("Ordering", type_ref::Target::Builtin, Vec::new());
            let (member, output) = if name == "Ord" {
                ("cmp", ordering)
            } else {
                (
                    "partial_cmp",
                    path(
                        "Option",
                        type_ref::Target::Builtin,
                        vec![type_ref::Argument::Type(ordering)],
                    ),
                )
            };
            vec![function(
                member,
                true,
                vec![("other", reference(false, this()))],
                Some(output),
            )]
        }
        "Copy" | "Eq" | "Hash" | "IntoFragment" => Vec::new(),
        // `trait Index<Idx> { type Output; fn index(&self, index: Idx) ->
        // &Self::Output; }` and `IndexMut<Idx>`'s `index_mut(&mut self, ..)
        // -> &mut Self::Output`.
        "Index" | "IndexMut" => {
            let output = type_ref::Type::Path(type_ref::Path {
                name: "Output".into(),
                target: type_ref::Target::Projection(Box::new(this()), Some(name.into())),
                arguments: Box::default(),
                span: Span::NONE,
            });
            let mutable = name == "IndexMut";
            let mut index = function(
                if mutable { "index_mut" } else { "index" },
                true,
                vec![(
                    "index",
                    path("Idx", type_ref::Target::Parameter, Vec::new()),
                )],
                Some(reference(mutable, output)),
            );
            if mutable {
                index.receiver = Some(crate::item::Receiver::Mutable(Span::NONE));
            }
            vec![index]
        }
        // `trait Iterator { type Item; fn next(&mut self) -> Option<Self::Item>; }`:
        // an impl's type gets the adaptors.
        "Iterator" => {
            let item = type_ref::Type::Path(type_ref::Path {
                name: "Item".into(),
                target: type_ref::Target::Projection(Box::new(this()), None),
                arguments: Box::default(),
                span: Span::NONE,
            });
            let mut next = function(
                "next",
                true,
                Vec::new(),
                Some(path(
                    "Option",
                    type_ref::Target::Builtin,
                    vec![type_ref::Argument::Type(item)],
                )),
            );
            next.receiver = Some(crate::item::Receiver::Mutable(Span::NONE));
            vec![next]
        }
        // `trait From<T> { fn from(value: T) -> Self; }`, and `Into<T>`,
        // which every `From` impl gives its source type.
        "From" => vec![function(
            "from",
            false,
            vec![("value", path("T", type_ref::Target::Parameter, Vec::new()))],
            Some(this()),
        )],
        // `trait FromIterator<A> { fn from_iter<I: IntoIterator<Item = A>>(iter: I)
        // -> Self; }`, which `collect()` into the type calls. Its `I` reads
        // as `impl IntoIterator<Item = A>`, as an impl's does.
        "FromIterator" => vec![function(
            "from_iter",
            false,
            vec![(
                "iter",
                path(
                    crate::item::type_ref::INTO_ITER_TYPE,
                    type_ref::Target::Builtin,
                    vec![type_ref::Argument::Type(path(
                        "A",
                        type_ref::Target::Parameter,
                        Vec::new(),
                    ))],
                ),
            )],
            Some(this()),
        )],
        "Into" => {
            let mut into = function(
                "into",
                false,
                Vec::new(),
                Some(path("T", type_ref::Target::Parameter, Vec::new())),
            );
            into.receiver = Some(crate::item::Receiver::Owned(Span::NONE));
            vec![into]
        }
        // `trait Add<Rhs = Self> { type Output; fn add(self, rhs: Rhs) ->
        // Self::Output; }` and its kin; `Neg` and `Not` take no `rhs`, and
        // `AddAssign<Rhs = Self>`'s `add_assign(&mut self, rhs: Rhs)` none.
        name if let Some((_, _, member)) = OPERATOR_TRAITS
            .iter()
            .find(|(_, trait_name, _)| *trait_name == name) =>
        {
            let rhs = path("Rhs", type_ref::Target::Parameter, Vec::new());
            let output = type_ref::Type::Path(type_ref::Path {
                name: "Output".into(),
                target: type_ref::Target::Projection(Box::new(this()), Some(name.into())),
                arguments: Box::default(),
                span: Span::NONE,
            });
            let mut operator = match name {
                "Neg" | "Not" => function(member, true, Vec::new(), Some(output)),
                _ if name.ends_with("Assign") => function(member, true, vec![("rhs", rhs)], None),
                _ => function(member, true, vec![("rhs", rhs)], Some(output)),
            };
            operator.receiver = Some(if name.ends_with("Assign") {
                crate::item::Receiver::Mutable(Span::NONE)
            } else {
                crate::item::Receiver::Owned(Span::NONE)
            });
            vec![operator]
        }
        _ => return None,
    };
    let operator = OPERATOR_TRAITS
        .iter()
        .any(|(_, trait_name, _)| *trait_name == name);
    let outputs = operator && !name.ends_with("Assign");
    let generics: Box<[item::Generic]> = if matches!(name, "Index" | "IndexMut") {
        Box::new([item::Generic {
            name: crate::item::Name {
                text: "Idx".into(),
                span: Span::NONE,
            },
            bounds: crate::capability::Set::new(),
            traits: Box::default(),
            projections: Box::default(),
            trait_arguments: Box::default(),
            default: None,
            host: false,
        }])
    } else if operator && !matches!(name, "Neg" | "Not") {
        Box::new([item::Generic {
            name: crate::item::Name {
                text: "Rhs".into(),
                span: Span::NONE,
            },
            bounds: crate::capability::Set::new(),
            traits: Box::default(),
            projections: Box::default(),
            trait_arguments: Box::default(),
            default: Some(this()),
            host: false,
        }])
    } else if matches!(name, "From" | "Into" | "FromIterator") {
        Box::new([item::Generic {
            name: crate::item::Name {
                text: if name == "FromIterator" { "A" } else { "T" }.into(),
                span: Span::NONE,
            },
            bounds: crate::capability::Set::new(),
            traits: Box::default(),
            projections: Box::default(),
            trait_arguments: Box::default(),
            default: None,
            host: false,
        }])
    } else {
        Box::default()
    };
    let provided = vec![false; functions.len()].into_boxed_slice();
    Some(item::Trait {
        header: crate::item::Header {
            attributes: Box::default(),
            visibility: None,
            span: Span::NONE,
        },
        name: crate::item::Name {
            text: name.into(),
            span: Span::NONE,
        },
        functions: functions.into_boxed_slice(),
        provided,
        associated: if name == "Iterator" || outputs || matches!(name, "Index" | "IndexMut") {
            Box::new([crate::item::Name {
                text: if name == "Iterator" { "Item" } else { "Output" }.into(),
                span: Span::NONE,
            }])
        } else {
            Box::default()
        },
        associated_bounds: if name == "Iterator" || outputs || matches!(name, "Index" | "IndexMut")
        {
            Box::new([(crate::capability::Set::new(), Box::default())])
        } else {
            Box::default()
        },
        associated_bound_arguments: if name == "Iterator"
            || outputs
            || matches!(name, "Index" | "IndexMut")
        {
            Box::new([Box::default()])
        } else {
            Box::default()
        },
        // `trait Ord: Eq + PartialOrd`.
        supertraits: if name == "Ord" {
            (
                crate::capability::Set::from_explicit([
                    crate::capability::Kind::PartialEq,
                    crate::capability::Kind::Eq,
                ]),
                Box::new([crate::item::Name {
                    text: "PartialOrd".into(),
                    span: Span::NONE,
                }]),
            )
        } else {
            (crate::capability::Set::new(), Box::default())
        },
        supertrait_arguments: Box::default(),
        generics,
        declaration: None,
    })
}

/// Every user trait named in a generic bound is a trait the crate declares.
fn check_trait_bounds(
    items: &[item::Shell],
    traits: &BTreeMap<ImmutableString, (program::source::Id, item::Trait)>,
) -> Result<(), LocatedError> {
    let check = |generics: &[item::Generic]| -> Result<(), Error> {
        for generic in generics {
            // Defaults are for a trait's own parameters here.
            if let Some(default) = &generic.default {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: "a default type parameter outside a trait declaration".into(),
                    source: type_span(default),
                });
            }
            for name in generic.traits.iter() {
                let Some((_, declared)) = traits.get(&name.text) else {
                    return Err(Error::UnknownType {
                        name: name.text.clone(),
                        reference: name.span,
                    });
                };
                // A generic trait takes one argument per parameter, its
                // defaults filled in already (E0107).
                let mut written = generic
                    .trait_arguments
                    .iter()
                    .filter(|(trait_name, _)| *trait_name == name.text)
                    .map(|(_, arguments)| arguments.len())
                    .collect::<Vec<_>>();
                if written.is_empty() {
                    written.push(0);
                }
                if let Some(actual) = written
                    .into_iter()
                    .find(|actual| *actual != declared.generics.len())
                {
                    return Err(Error::InvalidTypeArguments {
                        ty: name.text.clone(),
                        expected: declared.generics.len(),
                        actual,
                        reference: name.span,
                    });
                }
            }
        }
        Ok(())
    };
    for shell in items {
        let checked = match &shell.kind {
            item::Kind::Struct(item) => check(&item.generics),
            item::Kind::Enum(item) => check(&item.generics),
            item::Kind::Impl(item) => check(&item.generics).and_then(|()| {
                item.functions
                    .iter()
                    .try_for_each(|function| check(&function.generics))
            }),
            item::Kind::Trait(item) => item
                .functions
                .iter()
                .try_for_each(|function| check(&function.generics))
                .and_then(|()| {
                    // `type Output: Shape;` names a crate trait, as each
                    // supertrait does; a generic trait in either takes one
                    // argument per parameter, a supertrait's with its
                    // defaults filled in (E0107).
                    let associated = item
                        .associated_bounds
                        .iter()
                        .flat_map(|(_, traits)| traits)
                        .map(|name| (name, false));
                    let supers = item.supertraits.1.iter().map(|name| (name, true));
                    for (name, supertrait) in associated.chain(supers) {
                        let Some((_, declared)) = traits.get(&name.text) else {
                            return Err(Error::UnknownType {
                                name: name.text.clone(),
                                reference: name.span,
                            });
                        };
                        let mut written = if supertrait {
                            item.supertrait_arguments
                                .iter()
                                .filter(|(trait_name, _)| *trait_name == name.text)
                                .map(|(_, arguments)| arguments.len())
                                .collect::<Vec<_>>()
                        } else {
                            item.associated_bound_arguments
                                .iter()
                                .flatten()
                                .filter(|(trait_name, _)| *trait_name == name.text)
                                .map(|(_, arguments)| arguments.len())
                                .collect::<Vec<_>>()
                        };
                        if written.is_empty() {
                            written.push(0);
                        }
                        if let Some(actual) = written
                            .into_iter()
                            .find(|actual| *actual != declared.generics.len())
                        {
                            return Err(Error::InvalidTypeArguments {
                                ty: name.text.clone(),
                                expected: declared.generics.len(),
                                actual,
                                reference: name.span,
                            });
                        }
                    }
                    Ok(())
                }),
            item::Kind::Function(item) => check(&item.generics),
            item::Kind::Use(_) => Ok(()),
        };
        checked.map_err(|error| LocatedError::primary(shell.source, error))?;
    }
    Ok(())
}

/// Whether `ty` is the trait parameter `Self`.
fn is_self(ty: &type_ref::Type) -> bool {
    matches!(ty, type_ref::Type::Path(path)
        if path.name == "Self" && matches!(path.target, type_ref::Target::Parameter))
}

/// Each associated type an item header names exists, and is keyed by the
/// trait declaring it, as Rust resolves it: `T::Output` of the one trait
/// bounding `T` that declares `Output`, `Self::Output` of the trait being
/// declared, `Square::Output` of the one impl for `Square` giving it, and
/// `<T as Shape>::Output` of `Shape`. Two candidate traits are ambiguous,
/// as in Rust.
fn resolve_projections(items: &mut [item::Shell]) -> Result<(), LocatedError> {
    let declared: BTreeMap<ImmutableString, Box<[ImmutableString]>> = items
        .iter()
        .filter_map(|shell| match &shell.kind {
            item::Kind::Trait(declared) => Some((
                declared.name.text.clone(),
                declared
                    .associated
                    .iter()
                    .map(|name| name.text.clone())
                    .collect(),
            )),
            _ => None,
        })
        .collect();
    let mut implemented: BTreeMap<(type_def::Id, ImmutableString), Vec<ImmutableString>> =
        BTreeMap::new();
    for shell in items.iter() {
        let item::Kind::Impl(item) = &shell.kind else {
            continue;
        };
        let (
            Some(item::TraitImpl::User {
                name: trait_name, ..
            }),
            type_ref::Type::Path(owner),
        ) = (&item.trait_, &item.owner)
        else {
            continue;
        };
        let type_ref::Target::Script { definition, .. } = &owner.target else {
            continue;
        };
        for (name, _) in &item.associated {
            implemented
                .entry((*definition, name.text.clone()))
                .or_default()
                .push(trait_name.text.clone());
        }
    }
    fn walk(
        ty: &mut type_ref::Type,
        find: &mut dyn FnMut(&type_ref::Type, &type_ref::Path) -> Result<ImmutableString, Error>,
    ) -> Result<(), Error> {
        match ty {
            type_ref::Type::Unit(_) => Ok(()),
            type_ref::Type::Tuple { members, .. } => {
                members.iter_mut().try_for_each(|member| walk(member, find))
            }
            type_ref::Type::Reference { target, .. } => walk(target, find),
            type_ref::Type::Path(path) => {
                if let type_ref::Target::Projection(base, _) = &mut path.target {
                    walk(base, find)?;
                }
                let found = match &path.target {
                    type_ref::Target::Projection(base, _) => Some(find(base, path)?),
                    _ => None,
                };
                if let (Some(found), type_ref::Target::Projection(_, trait_name)) =
                    (found, &mut path.target)
                {
                    *trait_name = Some(found);
                }
                path.arguments
                    .iter_mut()
                    .try_for_each(|argument| match argument {
                        type_ref::Argument::Type(ty) => walk(ty, find),
                        type_ref::Argument::Lifetime(_) => Ok(()),
                    })
            }
        }
    }
    let resolve_item = |generics: &[item::Generic],
                        own_traits: &[ImmutableString],
                        types: &mut dyn Iterator<Item = &mut type_ref::Type>|
     -> Result<(), Error> {
        let mut find =
            |base: &type_ref::Type, path: &type_ref::Path| -> Result<ImmutableString, Error> {
                let type_ref::Target::Projection(_, written) = &path.target else {
                    unreachable!("only projections are resolved");
                };
                let type_ref::Type::Path(base_path) = base else {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: "an associated type of a type other than a generic parameter \
                                or a script type"
                            .into(),
                        source: path.span,
                    });
                };
                let declares = |trait_name: &ImmutableString| {
                    declared
                        .get(trait_name)
                        .is_some_and(|names| names.contains(&path.name))
                };
                let candidates: Vec<ImmutableString> = match &base_path.target {
                    // A trait's own associated types, then its supertraits'.
                    type_ref::Target::Parameter if base_path.name == "Self" => own_traits
                        .iter()
                        .find(|own| declares(own))
                        .cloned()
                        .into_iter()
                        .collect(),
                    type_ref::Target::Parameter => generics
                        .iter()
                        .filter(|generic| generic.name.text == base_path.name)
                        .flat_map(|generic| generic.traits.iter())
                        .map(|name| name.text.clone())
                        .filter(declares)
                        .collect(),
                    type_ref::Target::Script { definition, .. } => implemented
                        .get(&(*definition, path.name.clone()))
                        .cloned()
                        .unwrap_or_default(),
                    _ => Vec::new(),
                };
                match (written, &candidates[..]) {
                    (Some(written), _) if candidates.contains(written) => Ok(written.clone()),
                    (None, [found]) => Ok(found.clone()),
                    (None, [first, second, ..]) => Err(Error::InvalidMethodSignature {
                        owner: base_path.name.clone(),
                        method: path.name.clone(),
                        reason: format!(
                            "is an ambiguous associated type: both `{first}` and `{second}` \
                         declare `{name}`; write `<{base} as {first}>::{name}`",
                            name = path.name,
                            base = base_path.name,
                        )
                        .into(),
                        signature: path.span,
                    }),
                    (written, _) => Err(Error::UnknownMember {
                        owner: match written {
                            Some(written) => format!("<{} as {written}>", base_path.name).into(),
                            None => base_path.name.clone(),
                        },
                        member: path.name.clone(),
                        reference: path.span,
                    }),
                }
            };
        for ty in types {
            walk(ty, &mut find)?;
        }
        Ok(())
    };
    fn signature(function: &mut item::Function) -> impl Iterator<Item = &mut type_ref::Type> {
        function
            .parameters
            .iter_mut()
            .map(|parameter| &mut parameter.ty)
            .chain(function.output.iter_mut())
    }
    for shell in items.iter_mut() {
        let resolved = match &mut shell.kind {
            item::Kind::Function(function) => {
                let generics = function.generics.clone();
                resolve_item(&generics, &[], &mut signature(function))
            }
            item::Kind::Impl(item) => {
                let impl_generics = item.generics.clone();
                item.functions
                    .iter_mut()
                    .try_for_each(|function| {
                        let generics = impl_generics
                            .iter()
                            .chain(function.generics.iter())
                            .cloned()
                            .collect::<Vec<_>>();
                        resolve_item(&generics, &[], &mut signature(function))
                    })
                    .and_then(|()| {
                        resolve_item(
                            &impl_generics,
                            &[],
                            &mut item.associated.iter_mut().map(|(_, ty)| ty),
                        )
                    })
            }
            item::Kind::Trait(declared) => {
                let own = std::iter::once(declared.name.text.clone())
                    .chain(declared.supertraits.1.iter().map(|name| name.text.clone()))
                    .collect::<Vec<_>>();
                declared.functions.iter_mut().try_for_each(|function| {
                    let generics = function.generics.clone();
                    resolve_item(&generics, &own, &mut signature(function))
                })
            }
            item::Kind::Struct(item) => {
                let generics = item.generics.clone();
                resolve_item(
                    &generics,
                    &[],
                    &mut item.fields.iter_mut().map(|field| &mut field.ty),
                )
            }
            item::Kind::Enum(item) => {
                let generics = item.generics.clone();
                resolve_item(
                    &generics,
                    &[],
                    &mut item
                        .variants
                        .iter_mut()
                        .flat_map(|variant| variant.payload.iter_mut()),
                )
            }
            item::Kind::Use(_) => Ok(()),
        };
        resolved.map_err(|error| LocatedError::primary(shell.source, error))?;
    }
    Ok(())
}

/// `ty` with the trait parameter `Self` replaced by `owner`, and `Self::Name`
/// by the impl's associated type `Name` in `associated`.
fn substitute_self(
    ty: &type_ref::Type,
    owner: &type_ref::Type,
    associated: &[(crate::item::Name, type_ref::Type)],
    trait_name: &ImmutableString,
) -> type_ref::Type {
    match ty {
        _ if is_self(ty) => owner.clone(),
        type_ref::Type::Path(path)
            if let type_ref::Target::Projection(base, projected) = &path.target =>
        {
            if is_self(base)
                && projected
                    .as_ref()
                    .is_none_or(|projected| projected == trait_name)
                && let Some((_, bound)) = associated.iter().find(|(name, _)| name.text == path.name)
            {
                return bound.clone();
            }
            type_ref::Type::Path(type_ref::Path {
                target: type_ref::Target::Projection(
                    Box::new(substitute_self(base, owner, associated, trait_name)),
                    projected.clone(),
                ),
                ..path.clone()
            })
        }
        type_ref::Type::Path(path) => type_ref::Type::Path(type_ref::Path {
            arguments: path
                .arguments
                .iter()
                .map(|argument| match argument {
                    type_ref::Argument::Type(ty) => {
                        type_ref::Argument::Type(substitute_self(ty, owner, associated, trait_name))
                    }
                    type_ref::Argument::Lifetime(span) => type_ref::Argument::Lifetime(*span),
                })
                .collect(),
            ..path.clone()
        }),
        type_ref::Type::Tuple { members, span } => type_ref::Type::Tuple {
            members: members
                .iter()
                .map(|member| substitute_self(member, owner, associated, trait_name))
                .collect(),
            span: *span,
        },
        type_ref::Type::Reference {
            mutable,
            lifetime,
            target,
            span,
        } => type_ref::Type::Reference {
            mutable: *mutable,
            lifetime: *lifetime,
            target: Box::new(substitute_self(target, owner, associated, trait_name)),
            span: *span,
        },
        type_ref::Type::Unit(span) => type_ref::Type::Unit(*span),
    }
}

/// `ty` with each trait parameter in `parameters` replaced by its argument:
/// `T` by `i64` in `impl Convert<i64> for C`.
fn substitute_parameters(
    ty: &type_ref::Type,
    parameters: &[(ImmutableString, type_ref::Type)],
) -> type_ref::Type {
    if parameters.is_empty() {
        return ty.clone();
    }
    match ty {
        type_ref::Type::Path(path) if path.target == type_ref::Target::Parameter => parameters
            .iter()
            .find(|(name, _)| *name == path.name)
            .map_or_else(|| ty.clone(), |(_, argument)| argument.clone()),
        type_ref::Type::Path(path) => type_ref::Type::Path(type_ref::Path {
            arguments: path
                .arguments
                .iter()
                .map(|argument| match argument {
                    type_ref::Argument::Type(ty) => {
                        type_ref::Argument::Type(substitute_parameters(ty, parameters))
                    }
                    type_ref::Argument::Lifetime(span) => type_ref::Argument::Lifetime(*span),
                })
                .collect(),
            target: match &path.target {
                type_ref::Target::Projection(base, trait_name) => type_ref::Target::Projection(
                    Box::new(substitute_parameters(base, parameters)),
                    trait_name.clone(),
                ),
                target => target.clone(),
            },
            ..path.clone()
        }),
        type_ref::Type::Tuple { members, span } => type_ref::Type::Tuple {
            members: members
                .iter()
                .map(|member| substitute_parameters(member, parameters))
                .collect(),
            span: *span,
        },
        type_ref::Type::Reference {
            mutable,
            lifetime,
            target,
            span,
        } => type_ref::Type::Reference {
            mutable: *mutable,
            lifetime: *lifetime,
            target: Box::new(substitute_parameters(target, parameters)),
            span: *span,
        },
        type_ref::Type::Unit(span) => type_ref::Type::Unit(*span),
    }
}

/// The functions `impl Trait for Owner` provides, as Rust checks them: each
/// one a member of the trait with the trait's signature (`Self` being the
/// owner), every required one present, and each provided one the impl does
/// not override taken from the trait's default (`true`: from the trait).
fn trait_impl_functions(
    declared: &item::Trait,
    owner_name: &str,
    owner: &type_ref::Type,
    functions: &[item::Function],
    associated: &[(crate::item::Name, type_ref::Type)],
    parameters: &[(ImmutableString, type_ref::Type)],
    span: Span,
) -> Result<Vec<(item::Function, bool)>, Error> {
    // `Self` is the owner, and each trait parameter its argument.
    let substitute = |ty: &type_ref::Type| {
        substitute_parameters(
            &substitute_self(ty, owner, associated, &declared.name.text),
            parameters,
        )
    };
    // Each associated type the trait declares, and no other.
    for (name, _) in associated {
        if !declared
            .associated
            .iter()
            .any(|declared| declared.text == name.text)
        {
            return Err(Error::InvalidMethodSignature {
                owner: owner_name.into(),
                method: name.text.clone(),
                reason: format!("is not a member of trait `{}`", declared.name.text).into(),
                signature: name.span,
            });
        }
    }
    let receiver_kind = |function: &item::Function| {
        function.receiver.as_ref().map(|receiver| match receiver {
            crate::item::Receiver::Shared(_) => 0,
            crate::item::Receiver::Mutable(_) => 1,
            crate::item::Receiver::Owned(_) | crate::item::Receiver::OwnedMutable(_) => 2,
        })
    };
    let signature = |function: &item::Function, from_trait: bool| {
        let ty = |ty: &type_ref::Type| {
            if from_trait {
                canonical_type_name(&substitute(ty))
            } else {
                canonical_type_name(ty)
            }
        };
        let parameters = function
            .parameters
            .iter()
            .map(|parameter| ty(&parameter.ty))
            .collect::<Vec<_>>();
        let output = function
            .output
            .as_ref()
            .map_or_else(|| "()".to_string(), ty);
        (
            receiver_kind(function),
            parameters,
            output,
            function.generics.len(),
            function.body.constant,
        )
    };
    let mut implemented = Vec::new();
    for function in functions {
        let Some(required) = declared
            .functions
            .iter()
            .find(|required| required.name.text == function.name.text)
        else {
            return Err(Error::InvalidMethodSignature {
                owner: owner_name.into(),
                method: function.name.text.clone(),
                reason: format!("is not a member of trait `{}`", declared.name.text).into(),
                signature: function.name.span,
            });
        };
        if signature(function, false) != signature(required, true) {
            return Err(Error::InvalidMethodSignature {
                owner: owner_name.into(),
                method: function.name.text.clone(),
                reason: format!(
                    "has an incompatible type for trait `{}`: its signature differs from the \
                     trait's",
                    declared.name.text
                )
                .into(),
                signature: function.name.span,
            });
        }
        implemented.push((function.clone(), false));
    }
    let mut missing = declared
        .associated
        .iter()
        .filter(|required| {
            !associated
                .iter()
                .any(|(name, _)| name.text == required.text)
        })
        .map(|required| format!("`{}`", required.text))
        .collect::<Vec<_>>();
    for (required, provided) in declared.functions.iter().zip(declared.provided.iter()) {
        if functions
            .iter()
            .any(|function| function.name.text == required.name.text)
        {
            continue;
        }
        if !provided {
            missing.push(format!("`{}`", required.name.text));
            continue;
        }
        // The default body, checked and lowered for this owner.
        let mut function = required.clone();
        for parameter in function.parameters.iter_mut() {
            parameter.ty = substitute(&parameter.ty);
        }
        function.output = function.output.as_ref().map(substitute);
        implemented.push((function, true));
    }
    if !missing.is_empty() {
        return Err(Error::InvalidMethodSignature {
            owner: owner_name.into(),
            method: declared.name.text.clone(),
            reason: format!("is missing trait items: {}", missing.join(", ")).into(),
            signature: span,
        });
    }
    Ok(implemented)
}

fn resolve_functions(items: &mut [item::Shell]) -> Result<(), LocatedError> {
    for shell in items {
        let item::Kind::Function(function) = &mut shell.kind else {
            continue;
        };
        validate_method_signature("<module>", function, false, true)
            .map_err(|error| LocatedError::primary(shell.source, error))?;
        function.body.lowering = item::Lowering::NominalFunction;
    }
    Ok(())
}

/// The type names in scope inside an item declaring generics (when it
/// declares any), and its parameters with their bounds.
type GenericScope = (
    Option<BTreeMap<ImmutableString, type_ref::Target>>,
    Box<[item::Generic]>,
);

/// The type names in scope inside an item declaring `generics` (when it
/// declares any), and its parameters with their bounds. Rawr's bounds are
/// the built-in derivable traits; it has no user traits.
fn generic_scope(
    generics: Box<[crate::item::GenericParam]>,
    types: &BTreeMap<ImmutableString, type_ref::Target>,
    host: &host::Registry,
    #[cfg(feature = "rust-import")] native: Option<native::Site<'_>>,
) -> Result<GenericScope, Error> {
    if generics.is_empty() {
        return Ok((None, Box::default()));
    }
    let mut scope = types.clone();
    let mut resolved = Vec::with_capacity(generics.len());
    let mut names = BTreeMap::new();
    let mut projections = Vec::with_capacity(generics.len());
    let mut arguments = Vec::with_capacity(generics.len());
    let mut defaults = Vec::with_capacity(generics.len());
    for parameter in generics.into_vec() {
        projections.push(parameter.projections);
        defaults.push(parameter.default);
        if let Some(previous_declaration) =
            names.insert(parameter.name.text.clone(), parameter.name.span)
        {
            return Err(Error::DuplicateMember {
                owner: "<generics>".into(),
                member: parameter.name.text.clone(),
                declaration: parameter.name.span,
                previous_declaration,
            });
        }
        let (bounds, traits, trait_arguments) = resolve_bounds_with_arguments(&parameter.bounds)?;
        arguments.push(trait_arguments);
        scope.insert(parameter.name.text.clone(), type_ref::Target::Parameter);
        resolved.push(item::Generic {
            name: parameter.name,
            bounds,
            traits,
            projections: Box::default(),
            trait_arguments: Box::default(),
            default: None,
            host: false,
        });
    }
    // `trait Add<Rhs = Self>`: a default may name `Self` and the earlier
    // parameters.
    let mut default_scope = scope.clone();
    default_scope
        .entry("Self".into())
        .or_insert(type_ref::Target::Parameter);
    for (generic, default) in resolved.iter_mut().zip(defaults) {
        generic.default = default
            .map(|ty| {
                type_ref::resolve(
                    ty,
                    &default_scope,
                    host,
                    #[cfg(feature = "rust-import")]
                    native,
                )
            })
            .transpose()?;
    }
    // `T: Convert<U>`: each generic trait's arguments, with every parameter
    // in scope.
    for (generic, arguments) in resolved.iter_mut().zip(arguments) {
        let mut resolved_arguments = Vec::<item::TraitArgument>::new();
        for (trait_name, types) in arguments {
            let types = types
                .into_iter()
                .map(|ty| {
                    type_ref::resolve(
                        ty,
                        &scope,
                        host,
                        #[cfg(feature = "rust-import")]
                        native,
                    )
                })
                .collect::<Result<_, _>>()?;
            resolved_arguments.push((trait_name.text, types));
        }
        generic.trait_arguments = resolved_arguments.into_boxed_slice();
    }
    // `T: Shape<Output = U>`: each equality's type, with every parameter in
    // scope.
    for (generic, projections) in resolved.iter_mut().zip(projections) {
        generic.projections = projections
            .into_vec()
            .into_iter()
            .map(|(trait_name, name, ty)| {
                type_ref::resolve(
                    ty,
                    &scope,
                    host,
                    #[cfg(feature = "rust-import")]
                    native,
                )
                .map(|ty| (trait_name, name, ty))
            })
            .collect::<Result<_, _>>()?;
    }
    Ok((Some(scope), resolved.into_boxed_slice()))
}

/// The arguments the user traits of a bound list take, unresolved:
/// `(Convert, [i64])` of `Convert<i64>`.
type BoundArguments = Vec<(crate::item::Name, Vec<crate::item::type_ref::Type>)>;

/// The capabilities and user traits of a bound list (`Clone + Shape`),
/// with each generic trait's type arguments.
fn resolve_bounds_with_arguments(
    bounds_written: &[crate::item::type_ref::Path],
) -> Result<
    (
        crate::capability::Set,
        Box<[crate::item::Name]>,
        BoundArguments,
    ),
    Error,
> {
    let mut bounds = Vec::new();
    let mut traits = Vec::new();
    let mut trait_arguments = Vec::new();
    for bound in bounds_written {
        // `Send`, `Sync` and the other auto traits, bare or by their std
        // path. rawr does not prove a type's auto traits, so the bound is
        // accepted and asks nothing (an over-admission: an `Rc` passes
        // `T: Send`), as rustc reads it for every type that has them.
        if crate::parser::item::is_auto_trait(bound) {
            continue;
        }
        // `Debug`, or `std::fmt::Debug` as Rust spells it in full.
        let kind = match &bound.segments[..] {
            [prefix @ .., segment]
                if segment.arguments.is_empty()
                    && prefix.iter().all(|module| {
                        module.arguments.is_empty()
                            && matches!(
                                module.name.text.as_str(),
                                "std"
                                    | "core"
                                    | "fmt"
                                    | "clone"
                                    | "cmp"
                                    | "default"
                                    | "marker"
                                    | "hash"
                            )
                    }) =>
            {
                crate::capability::Kind::from_name(&segment.name.text)
            }
            _ => None,
        };
        // `PartialOrd: PartialEq` and `Ord: Eq`, as in Rust.
        match bound
            .segments
            .last()
            .map(|segment| segment.name.text.as_str())
        {
            Some("PartialOrd") if kind.is_none() => bounds.push(crate::capability::Kind::PartialEq),
            Some("Ord") if kind.is_none() => bounds.extend([
                crate::capability::Kind::PartialEq,
                crate::capability::Kind::Eq,
            ]),
            _ => {}
        }
        let Some(kind) = kind else {
            // A user trait, by name; it is checked against the crate's
            // trait declarations once every item is resolved.
            if let [segment] = &bound.segments[..]
                && segment.arguments.is_empty()
            {
                traits.push(segment.name.clone());
                continue;
            }
            // `Convert<i64>`: a generic user trait and its arguments.
            if let [segment] = &bound.segments[..]
                && !matches!(
                    segment.name.text.as_str(),
                    "Fn" | "FnMut" | "FnOnce" | crate::item::type_ref::FN_TYPE
                )
                && segment
                    .arguments
                    .iter()
                    .all(|argument| matches!(argument, crate::item::type_ref::Argument::Type(_)))
            {
                traits.push(segment.name.clone());
                trait_arguments.push((
                    segment.name.clone(),
                    segment
                        .arguments
                        .iter()
                        .filter_map(|argument| match argument {
                            crate::item::type_ref::Argument::Type(ty) => Some(ty.clone()),
                            crate::item::type_ref::Argument::Lifetime(_) => None,
                        })
                        .collect(),
                ));
                continue;
            }
            if let Some(name) = std_trait(&bound.segments) {
                traits.push(name);
                continue;
            }
            // iced's `text::IntoFragment<'a>`, what `text(..)` takes: Rust's
            // strings, `char`, `bool` and numbers, by reference too.
            if let [prefix @ .., last] = &bound.segments[..]
                && last.name.text == "IntoFragment"
                && last.arguments.iter().all(|argument| {
                    matches!(argument, crate::item::type_ref::Argument::Lifetime(_))
                })
                && prefix.iter().all(|module| {
                    module.arguments.is_empty()
                        && matches!(
                            module.name.text.as_str(),
                            "iced" | "core" | "advanced" | "widget" | "text"
                        )
                })
            {
                traits.push(last.name.clone());
                continue;
            }
            // iced's `advanced` traits by their module path:
            // `renderer::Renderer`, `advanced::widget::Widget<M, T, R>`.
            if let [prefix @ .., last] = &bound.segments[..]
                && !prefix.is_empty()
                && prefix.iter().all(|module| {
                    module.arguments.is_empty()
                        && matches!(
                            module.name.text.as_str(),
                            "iced"
                                | "core"
                                | "advanced"
                                | "widget"
                                | "renderer"
                                | "graphics"
                                | "mesh"
                        )
                })
                && matches!(last.name.text.as_str(), "Renderer" | "Widget" | "Meta")
                && last
                    .arguments
                    .iter()
                    .all(|argument| matches!(argument, crate::item::type_ref::Argument::Type(_)))
            {
                traits.push(last.name.clone());
                if !last.arguments.is_empty() {
                    trait_arguments.push((
                        last.name.clone(),
                        last.arguments
                            .iter()
                            .filter_map(|argument| match argument {
                                crate::item::type_ref::Argument::Type(ty) => Some(ty.clone()),
                                crate::item::type_ref::Argument::Lifetime(_) => None,
                            })
                            .collect(),
                    ));
                }
                continue;
            }
            return Err(Error::UnsupportedNominalSyntax {
                construct: "a trait bound path other than a built-in trait or a trait \
                            declared in the crate"
                    .into(),
                source: bound.span,
            });
        };
        bounds.push(kind);
        // `Copy` implies `Clone`, and `Eq` implies `PartialEq`, as in Rust.
        match kind {
            crate::capability::Kind::Copy => bounds.push(crate::capability::Kind::Clone),
            crate::capability::Kind::Eq => bounds.push(crate::capability::Kind::PartialEq),
            _ => {}
        }
    }
    Ok((
        crate::capability::Set::from_explicit(bounds),
        traits.into_boxed_slice(),
        trait_arguments,
    ))
}

/// The trait of `impl Trait for Type`: Rawr recognises `From<T>`, the
/// conversion `.into()` and `?` use.
fn resolve_trait(
    path: crate::item::type_ref::Path,
    types: &BTreeMap<ImmutableString, type_ref::Target>,
    host: &crate::host::Registry,
    #[cfg(feature = "rust-import")] native: Option<native::Site<'_>>,
) -> Result<item::TraitImpl, Error> {
    let span = path.span;
    // `impl Trait for Type` of a trait declared in the crate, by name, or
    // of a facade trait script types may implement, by its declared name
    // through the scope (`impl component::Component<'a, M> for C`); an
    // unimported name is unknown once every item is resolved.
    // `impl Convert<i64> for C`: a generic trait's arguments.
    if let Some(last) = path.segments.last()
        && (path.segments.len() == 1
            || type_ref::host_trait(&path, types, host).is_some_and(|definition| {
                matches!(definition.shape(), host::type_def::Shape::Trait(_))
            }))
    {
        let arguments = last
            .arguments
            .iter()
            .filter_map(|argument| match argument {
                crate::item::type_ref::Argument::Type(ty) => Some(type_ref::resolve(
                    ty.clone(),
                    types,
                    host,
                    #[cfg(feature = "rust-import")]
                    native,
                )),
                crate::item::type_ref::Argument::Lifetime(_) => None,
            })
            .collect::<Result<_, _>>()?;
        return Ok(item::TraitImpl::User {
            name: last.name.clone(),
            arguments,
        });
    }
    if let Some(name) = std_trait(&path.segments) {
        return Ok(item::TraitImpl::User {
            name,
            arguments: Box::default(),
        });
    }
    // `std::convert::From<T>` and `std::iter::FromIterator<A>`: the
    // prelude's conversion traits by path.
    if let [prefix @ .., last] = &path.segments[..]
        && matches!(last.name.text.as_str(), "From" | "Into" | "FromIterator")
        && !prefix.is_empty()
        && prefix.iter().all(|module| {
            module.arguments.is_empty()
                && matches!(
                    module.name.text.as_str(),
                    "std" | "core" | "convert" | "iter"
                )
        })
    {
        let arguments = last
            .arguments
            .iter()
            .filter_map(|argument| match argument {
                crate::item::type_ref::Argument::Type(ty) => Some(type_ref::resolve(
                    ty.clone(),
                    types,
                    host,
                    #[cfg(feature = "rust-import")]
                    native,
                )),
                crate::item::type_ref::Argument::Lifetime(_) => None,
            })
            .collect::<Result<_, _>>()?;
        return Ok(item::TraitImpl::User {
            name: last.name.clone(),
            arguments,
        });
    }
    Err(Error::UnsupportedNominalSyntax {
        construct: "an `impl` of a trait path that names no crate or std trait".into(),
        source: span,
    })
}

/// The error for `dyn Trait` at `span` when the trait `declared` is not
/// dyn compatible by Rust's rules; `None` when it is.
pub(crate) fn dyn_incompatibility(
    declared: &item::Trait,
    bindings: &[ImmutableString],
    span: Span,
    traits: &BTreeMap<ImmutableString, (program::source::Id, item::Trait)>,
) -> Option<Error> {
    // Each supertrait must be dyn compatible too; `Clone`, `Copy` and
    // `Default` need `Self: Sized`, and `PartialEq`/`Eq` name `Self`.
    let name = &declared.name.text;
    let (bounds, supers) = &declared.supertraits;
    if let Some(kind) = bounds.iter().find(|kind| {
        matches!(
            kind,
            crate::capability::Kind::Clone
                | crate::capability::Kind::Copy
                | crate::capability::Kind::Default
                | crate::capability::Kind::PartialEq
                | crate::capability::Kind::Eq
        )
    }) {
        return Some(Error::InvalidMethodSignature {
            owner: name.clone(),
            method: kind.name().into(),
            reason: format!(
                "the trait `{name}` is not dyn compatible, so `dyn {name}` is not a type: its \
                 supertrait `{}` requires `Self: Sized` or uses `Self` as a type parameter",
                kind.name()
            )
            .into(),
            signature: span,
        });
    }
    for parent in supers.iter() {
        let Some((_, parent)) = traits.get(&parent.text) else {
            continue;
        };
        if let Some(associated) = parent.associated.first() {
            return Some(Error::UnsupportedNominalSyntax {
                construct: format!(
                    "`dyn {name}` whose supertrait `{}` has the associated type `{}`",
                    parent.name.text, associated.text
                )
                .into(),
                source: span,
            });
        }
        let mut own = parent.clone();
        own.supertraits = (crate::capability::Set::new(), Box::default());
        if let Some(error) = dyn_incompatibility(&own, &[], span, traits) {
            return Some(error);
        }
    }
    dyn_own_incompatibility(declared, bindings, span)
}

/// `dyn_incompatibility` of the trait's own items.
fn dyn_own_incompatibility(
    declared: &item::Trait,
    bindings: &[ImmutableString],
    span: Span,
) -> Option<Error> {
    fn mentions_self(ty: &type_ref::Type) -> bool {
        match ty {
            type_ref::Type::Unit(_) => false,
            type_ref::Type::Tuple { members, .. } => members.iter().any(mentions_self),
            type_ref::Type::Reference { target, .. } => mentions_self(target),
            type_ref::Type::Path(path) => {
                (path.name == "Self" && path.target == type_ref::Target::Parameter)
                    || path.arguments.iter().any(|argument| match argument {
                        type_ref::Argument::Type(ty) => mentions_self(ty),
                        type_ref::Argument::Lifetime(_) => false,
                    })
            }
        }
    }
    let name = &declared.name.text;
    let error = |member: &ImmutableString, reason: &str| Error::InvalidMethodSignature {
        owner: name.clone(),
        method: member.clone(),
        reason: format!(
            "the trait `{name}` is not dyn compatible, so `dyn {name}` is not a type: {reason}"
        )
        .into(),
        signature: span,
    };
    // `dyn Shape<Output = i64>`: every associated type is given (E0191),
    // and only those.
    if let Some(associated) = declared
        .associated
        .iter()
        .find(|associated| !bindings.contains(&associated.text))
    {
        return Some(Error::InvalidMethodSignature {
            owner: name.clone(),
            method: associated.text.clone(),
            reason: format!(
                "the value of the associated type `{}` in `{name}` must be specified: write \
                 `dyn {name}<{} = ..>` (E0191)",
                associated.text, associated.text
            )
            .into(),
            signature: span,
        });
    }
    if let Some(unknown) = bindings.iter().find(|binding| {
        !declared
            .associated
            .iter()
            .any(|name| name.text == **binding)
    }) {
        return Some(Error::UnknownMember {
            owner: name.clone(),
            member: unknown.clone(),
            reference: span,
        });
    }
    for function in &declared.functions {
        let member = &function.name.text;
        if function.body.constant {
            return Some(error(member, "it contains this associated const"));
        }
        if function.receiver.is_none() {
            return Some(error(
                member,
                "this associated function has no `self` parameter",
            ));
        }
        if !function.generics.is_empty() {
            return Some(error(member, "this method has generic type parameters"));
        }
        if function
            .parameters
            .iter()
            .any(|parameter| mentions_self(&parameter.ty))
            || function.output.as_ref().is_some_and(mentions_self)
        {
            return Some(error(
                member,
                "this method references the `Self` type in its parameters or return type",
            ));
        }
    }
    None
}

/// Each use of a script type in an item header gives it exactly as many
/// type arguments as it declares generic parameters, as Rust requires; each
/// `dyn Trait` names a crate trait that can be made into an object.
fn check_generic_arity(
    items: &[item::Shell],
    traits: &BTreeMap<ImmutableString, (program::source::Id, item::Trait)>,
) -> Result<(), LocatedError> {
    let arity = items
        .iter()
        .filter_map(|shell| match &shell.kind {
            item::Kind::Struct(item) => Some((item.definition, item.generics.len())),
            item::Kind::Enum(item) => Some((item.definition, item.generics.len())),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    fn check(
        ty: &type_ref::Type,
        arity: &BTreeMap<type_def::Id, usize>,
        traits: &BTreeMap<ImmutableString, (program::source::Id, item::Trait)>,
    ) -> Result<(), Error> {
        match ty {
            type_ref::Type::Unit(_) => Ok(()),
            type_ref::Type::Tuple { members, .. } => members
                .iter()
                .try_for_each(|member| check(member, arity, traits)),
            type_ref::Type::Reference { target, .. } => check(target, arity, traits),
            type_ref::Type::Path(path) if let Some(name) = ty.dyn_trait() => {
                let Some((_, declared)) = traits.get(name) else {
                    return Err(Error::UnknownType {
                        name: name.into(),
                        reference: path.span,
                    });
                };
                let bindings = ty.dyn_bindings();
                let names = bindings
                    .iter()
                    .map(|(name, _)| name.clone())
                    .collect::<Vec<_>>();
                if let Some(error) = dyn_incompatibility(declared, &names, path.span, traits) {
                    return Err(error);
                }
                // `dyn Convert<i64>`: one argument per trait parameter.
                let positional = ty.dyn_arguments();
                if positional.len() != declared.generics.len() {
                    return Err(Error::InvalidTypeArguments {
                        ty: name.into(),
                        expected: declared.generics.len(),
                        actual: positional.len(),
                        reference: path.span,
                    });
                }
                positional
                    .into_iter()
                    .chain(bindings.into_iter().map(|(_, ty)| ty))
                    .try_for_each(|ty| check(ty, arity, traits))
            }
            type_ref::Type::Path(path) => {
                let arguments = path
                    .arguments
                    .iter()
                    .filter_map(|argument| match argument {
                        type_ref::Argument::Type(ty) => Some(ty),
                        type_ref::Argument::Lifetime(_) => None,
                    })
                    .collect::<Vec<_>>();
                if let type_ref::Target::Script { definition, .. } = &path.target
                    && let Some(expected) = arity.get(definition)
                    && *expected != arguments.len()
                {
                    return Err(Error::InvalidTypeArguments {
                        ty: path.name.clone(),
                        expected: *expected,
                        actual: arguments.len(),
                        reference: path.span,
                    });
                }
                arguments
                    .into_iter()
                    .try_for_each(|ty| check(ty, arity, traits))
            }
        }
    }
    let function = |function: &item::Function| -> Result<(), Error> {
        function
            .parameters
            .iter()
            .try_for_each(|parameter| check(&parameter.ty, &arity, traits))?;
        function
            .output
            .as_ref()
            .map_or(Ok(()), |output| check(output, &arity, traits))
    };
    for shell in items {
        let checked = match &shell.kind {
            item::Kind::Struct(item) => item
                .fields
                .iter()
                .try_for_each(|field| check(&field.ty, &arity, traits)),
            item::Kind::Enum(item) => item.variants.iter().try_for_each(|variant| {
                variant
                    .payload
                    .iter()
                    .try_for_each(|ty| check(ty, &arity, traits))
            }),
            item::Kind::Impl(item) => check(&item.owner, &arity, traits)
                .and_then(|()| item.functions.iter().try_for_each(function)),
            item::Kind::Trait(item) => item.functions.iter().try_for_each(function),
            item::Kind::Function(item) => function(item),
            item::Kind::Use(_) => Ok(()),
        };
        checked.map_err(|error| LocatedError::primary(shell.source, error))?;
    }
    Ok(())
}

/// The type arguments of an impl's owner: `T` of `impl<T> Pair<T>`.
fn owner_arguments(ty: &type_ref::Type) -> Box<[type_ref::Type]> {
    let type_ref::Type::Path(path) = ty else {
        return Box::default();
    };
    path.arguments
        .iter()
        .filter_map(|argument| match argument {
            type_ref::Argument::Type(ty) => Some(ty.clone()),
            type_ref::Argument::Lifetime(_) => None,
        })
        .collect()
}

fn impl_owner(ty: &type_ref::Type) -> Result<(type_def::Id, ImmutableString), Error> {
    let type_ref::Type::Path(path) = ty else {
        return Err(Error::InvalidImplOwner {
            owner: type_name(ty),
            reference: type_span(ty),
        });
    };
    let type_ref::Target::Script { definition, .. } = path.target else {
        return Err(Error::InvalidImplOwner {
            owner: path.name.clone(),
            reference: path.span,
        });
    };
    Ok((definition, path.name.clone()))
}

fn validate_method_signature(
    owner: &str,
    function: &item::Function,
    widget_protocol: bool,
    copied: bool,
) -> Result<(), Error> {
    let preview_subject = function
        .header
        .attributes
        .iter()
        .any(|attribute| attribute.preview().is_some());
    // A free function's one `&mut` parameter is its receiver at runtime
    // (`lower_nominal::this_parameter`).
    let mut receiver_free = owner == "<module>";
    for parameter in &function.parameters {
        if let type_ref::Type::Reference { mutable, .. } = parameter.ty {
            // Invocation-scoped references are validated once the full program and
            // widget marker context are available below.
            if direct_mutable_invocation(&parameter.ty) {
                continue;
            }
            if widget_protocol {
                continue;
            }
            // Shared borrows are passed by value, as an iced style
            // function's `&Theme`.
            if !mutable {
                continue;
            }
            // Further `&mut` parameters of a free function or an inherent
            // method are copied in and back out by the call
            // (`lower_nominal::copied_parameters`).
            if std::mem::take(&mut receiver_free) || copied {
                continue;
            }
            return Err(Error::InvalidMethodSignature {
                owner: owner.into(),
                method: function.name.text.clone(),
                reason: if preview_subject {
                    "preview function inputs may use only shared references".into()
                } else {
                    "mutable references are supported only for `self` receivers and the \
                     parameters of free functions and inherent methods"
                        .into()
                },
                signature: type_span(&parameter.ty),
            });
        }
    }
    // A `const` of a reference type (`const ALL: &[T] = &[..]`) is its
    // value, as references are at runtime; so is a shared reference a
    // function returns (`fn name(&self) -> &String`, `Index::index`). A
    // returned `&mut` would be a copy, which Rust's aliasing would not be.
    if !function.body.constant
        && function
            .output
            .as_ref()
            .is_some_and(|output| matches!(output, type_ref::Type::Reference { mutable: true, .. }))
        // `IndexMut::index_mut` is declared; changing through it is not.
        && function.name.text != "index_mut"
    {
        let output = function.output.as_ref().expect("checked output exists");
        return Err(Error::InvalidMethodSignature {
            owner: owner.into(),
            method: function.name.text.clone(),
            reason: "method outputs cannot borrow values in this phase".into(),
            signature: type_span(output),
        });
    }
    Ok(())
}

/// A type as Rust spells it, generic arguments included.
pub(crate) fn full_type_name(ty: &type_ref::Type) -> String {
    type_name_of(ty, false)
}

/// `full_type_name` with each host type by its facade path, as a trait's
/// signature and an impl's compare however each imports the type.
fn canonical_type_name(ty: &type_ref::Type) -> String {
    type_name_of(ty, true)
}

fn type_name_of(ty: &type_ref::Type, canonical: bool) -> String {
    let name = |ty| type_name_of(ty, canonical);
    match ty {
        type_ref::Type::Unit(_) => "()".into(),
        type_ref::Type::Tuple { members, .. } => format!(
            "({})",
            members.iter().map(name).collect::<Vec<_>>().join(", ")
        ),
        type_ref::Type::Path(path) => {
            let arguments = path
                .arguments
                .iter()
                .filter_map(|argument| match argument {
                    type_ref::Argument::Type(ty) => Some(name(ty)),
                    type_ref::Argument::Lifetime(_) => None,
                })
                .collect::<Vec<_>>();
            let written = match &path.target {
                type_ref::Target::Host { definition, .. } if canonical => definition.path(),
                _ => path.name.to_string(),
            };
            if let type_ref::Target::Projection(base, _) = &path.target {
                format!("{}::{}", name(base), path.name)
            } else if arguments.is_empty() {
                written
            } else {
                format!("{written}<{}>", arguments.join(", "))
            }
        }
        type_ref::Type::Reference {
            mutable, target, ..
        } => format!("&{}{}", if *mutable { "mut " } else { "" }, name(target)),
    }
}

fn type_span(ty: &type_ref::Type) -> Span {
    match ty {
        type_ref::Type::Unit(span)
        | type_ref::Type::Tuple { span, .. }
        | type_ref::Type::Reference { span, .. } => *span,
        type_ref::Type::Path(path) => path.span,
    }
}

fn type_name(ty: &type_ref::Type) -> ImmutableString {
    match ty {
        type_ref::Type::Unit(_) => "()".into(),
        type_ref::Type::Tuple { members, .. } => format!(
            "({})",
            members.iter().map(type_name).collect::<Vec<_>>().join(", ")
        )
        .into(),
        type_ref::Type::Path(path) => path.name.clone(),
        type_ref::Type::Reference { target, .. } => type_name(target),
    }
}

fn declared_type(
    name: &crate::item::Name,
    types: &BTreeMap<ImmutableString, type_ref::Target>,
) -> (type_def::Id, type_def::Key) {
    let target = types
        .get(name.text.as_str())
        .expect("predeclared nominal types retain their definition identity");

    match target {
        type_ref::Target::Script {
            definition,
            identity: type_def::Identity::Script(key),
        } => (*definition, key.clone()),
        type_ref::Target::Builtin
        | type_ref::Target::Host { .. }
        | type_ref::Target::Parameter
        | type_ref::Target::Projection(..)
        | type_ref::Target::Alias(_)
        | type_ref::Target::Script {
            identity: type_def::Identity::Host(_),
            ..
        } => unreachable!("declared nominal types always have script identities"),
    }
}

#[cfg(test)]
mod tests {
    use super::item::Kind;
    use super::type_ref::{Target, Type};
    use super::{resolve, resolve_with, AllocationOrder, Category, Error, Namespace, Program};
    use crate::{host, program, type_def, Engine};
    use std::any::TypeId;

    #[test]
    fn spawn_blocking_stays_rawr_iced_s_in_a_program_that_imports_tokio() {
        fn facade(root: &str, modules: &[&str], items: &[&str]) -> host::facade::Definition {
            let mut builder =
                host::facade::Builder::new(root, host::facade::Abi::new(1).unwrap()).unwrap();
            for module in modules {
                builder.module(*module).unwrap();
            }
            for item in items {
                let mut signature = host::signature::Builder::new();
                signature.output(host::type_ref::Type::builtin(host::type_ref::Builtin::I64));
                builder
                    .item(host::item::Definition::function(*item, signature.finish()).unwrap())
                    .unwrap();
            }
            builder.finish().unwrap()
        }
        let mut engine = Engine::new();
        engine
            .register_host_facade(facade(
                "rawr_iced",
                &["tokio", "tokio::task"],
                &["tokio::task::spawn_blocking", "tokio::task::yield_now"],
            ))
            .unwrap();
        assert_eq!(
            super::std_reexport("tokio::task::yield_now", &engine.host_registry).as_str(),
            "rawr_iced::tokio::task::yield_now"
        );
        engine
            .register_host_facade(facade("tokio", &["task"], &["task::block_in_place"]))
            .unwrap();
        let host = &engine.host_registry;
        assert_eq!(
            super::std_reexport("tokio::task::spawn_blocking", host).as_str(),
            "rawr_iced::tokio::task::spawn_blocking"
        );
        // The imported tokio keeps its own items, and rawr-iced's others
        // are not reached.
        assert_eq!(
            super::std_reexport("tokio::task::block_in_place", host).as_str(),
            "tokio::task::block_in_place"
        );
        assert_eq!(
            super::std_reexport("tokio::task::yield_now", host).as_str(),
            "tokio::task::yield_now"
        );
    }

    #[test]
    fn forward_item_names_resolve_in_headers() {
        let engine = Engine::new();
        let unit = engine
            .parse_unit("struct Envelope { message: Message } enum Message { Tick, Move(i64) }")
            .unwrap();
        let program = resolve(unit, &host::Registry::new()).unwrap();

        let Kind::Struct(envelope) = &program.items()[0].kind else {
            panic!("expected resolved struct");
        };
        let Kind::Enum(message) = &program.items()[1].kind else {
            panic!("expected resolved enum");
        };
        let Type::Path(message_field) = &envelope.fields[0].ty else {
            panic!("expected resolved path");
        };

        assert!(matches!(
            &message_field.target,
            Target::Script {
                definition,
                identity: type_def::Identity::Script(key),
            } if *definition == message.definition && key == &message.key
        ));
        assert_eq!(program.type_count(), 2);
        assert_ne!(program.items()[0].id, program.items()[1].id);
        assert_eq!(program.types()[1].id(), message.definition);
    }

    #[test]
    fn duplicate_items_report_both_declaration_spans() {
        let engine = Engine::new();
        let unit = engine
            .parse_unit("struct Counter;\nstruct Counter;")
            .unwrap();
        let Err(error) = resolve(unit, &host::Registry::new()) else {
            panic!("expected duplicate item error");
        };

        assert_eq!(error.category(), Category::DuplicateItem);
        assert_eq!(error.name(), "Counter");
        assert_eq!(error.primary_span().start().line(), Some(2));
        assert_eq!(error.secondary_span().unwrap().start().line(), Some(1));
        assert!(matches!(error, Error::DuplicateItem { .. }));
    }

    #[test]
    fn unknown_header_types_report_name_position_and_category() {
        let engine = Engine::new();
        let unit = engine
            .parse_unit("struct Envelope {\n    message: Missing,\n}")
            .unwrap();
        let Err(error) = resolve(unit, &host::Registry::new()) else {
            panic!("expected unknown type error");
        };

        assert_eq!(error.category(), Category::UnknownType);
        assert_eq!(error.name(), "Missing");
        assert_eq!(error.primary_span().start().line(), Some(2));
        assert_eq!(error.primary_span().start().position(), Some(14));
        assert_eq!(error.secondary_span(), None);
        assert!(error.to_string().contains("unknown type `Missing`"));
    }

    #[test]
    fn registered_host_types_resolve_with_script_arguments() {
        let engine = Engine::new();
        let unit = engine
            .parse_unit(
                "enum Message { Tick } pub fn view() -> iced::widget::Column<'_, Message> { () }",
            )
            .unwrap();
        let mut registry = host::Registry::new();
        registry
            .register_type::<Column>("iced::widget::Column")
            .unwrap();
        let program = resolve(unit, &registry).unwrap();

        let Kind::Function(view) = &program.items()[1].kind else {
            panic!("expected resolved function");
        };
        let Some(Type::Path(output)) = &view.output else {
            panic!("expected resolved function output");
        };

        assert!(matches!(
            &output.target,
            Target::Host { definition, .. }
                if definition.native_identity() == Some(TypeId::of::<Column>())
        ));
        assert_eq!(output.arguments.len(), 2);
    }

    #[test]
    fn staged_headers_resolve_every_nominal_reference_to_stable_identity() {
        let engine = Engine::new();
        let unit = engine
            .parse_unit(
                "struct State { event: Event } enum Event { Replace(State) } impl State { fn update(&mut self, event: Event) -> State { () } } pub fn boot(event: Event) -> State { () }",
            )
            .unwrap();
        let program = resolve(unit, &host::Registry::new()).unwrap();
        let state_identity =
            type_def::Identity::Script(definition(&program, "State").key().clone());
        let event_identity =
            type_def::Identity::Script(definition(&program, "Event").key().clone());

        let Kind::Struct(state) = &program.items()[0].kind else {
            panic!("expected struct");
        };
        assert_eq!(path_identity(&state.fields[0].ty), &event_identity);

        let Kind::Enum(event) = &program.items()[1].kind else {
            panic!("expected enum");
        };
        assert_eq!(
            path_identity(&event.variants[0].payload[0]),
            &state_identity
        );

        let Kind::Impl(state_impl) = &program.items()[2].kind else {
            panic!("expected impl");
        };
        assert_eq!(path_identity(&state_impl.owner), &state_identity);
        assert_eq!(
            path_identity(&state_impl.functions[0].parameters[0].ty),
            &event_identity
        );
        assert_eq!(
            path_identity(state_impl.functions[0].output.as_ref().unwrap()),
            &state_identity
        );

        let Kind::Function(boot) = &program.items()[3].kind else {
            panic!("expected function");
        };
        assert_eq!(path_identity(&boot.parameters[0].ty), &event_identity);
        assert_eq!(
            path_identity(boot.output.as_ref().unwrap()),
            &state_identity
        );
    }

    #[test]
    fn resolver_owns_ids_and_the_statements_outside_items() {
        let engine = Engine::new();
        let unit = engine
            .parse_unit("struct Counter; let answer = 40 + 2; answer")
            .unwrap();
        let program = resolve(unit, &host::Registry::new()).unwrap();

        assert_eq!(program.item_count(), 1);
        assert_eq!(program.types()[0].item(), program.items()[0].id);
        assert_eq!(program.types()[0].name(), "Counter");
        assert_eq!(
            program.source().text(),
            "struct Counter; let answer = 40 + 2; answer"
        );
    }

    #[test]
    fn typed_function_bodies_are_marked_for_nominal_lowering() {
        let engine = Engine::new();
        let unit = engine.parse_unit("pub fn work() { 40 + 2 }").unwrap();
        let program = resolve(unit, &host::Registry::new()).unwrap();

        let Kind::Function(function) = &program.items()[0].kind else {
            panic!("expected resolved function");
        };

        assert_eq!(
            function.body.lowering,
            super::item::Lowering::NominalFunction
        );
        assert!(!function.body.span.is_none());
    }

    #[test]
    fn stable_keys_and_schemas_ignore_dense_allocation_order() {
        let engine = Engine::new();
        let source = "struct Envelope { message: Message } enum Message { Tick, Move(i64) } pub fn consume(value: Envelope) {}";
        let namespace = Namespace::new("counter", &["model"]);
        let host = host::Registry::new();
        let source_order = resolve_with(
            engine.parse_unit(source).unwrap(),
            &host,
            &namespace,
            AllocationOrder::Source,
        )
        .unwrap();
        let reverse_order = resolve_with(
            engine.parse_unit(source).unwrap(),
            &host,
            &namespace,
            AllocationOrder::Reverse,
        )
        .unwrap();

        for name in ["Envelope", "Message"] {
            let source_definition = definition(&source_order, name);
            let reverse_definition = definition(&reverse_order, name);

            assert_eq!(source_definition.key(), reverse_definition.key());
            assert_eq!(source_definition.schema(), reverse_definition.schema());
            assert_ne!(source_definition.id(), reverse_definition.id());

            let source_value = nominal_value(&source_order, name);
            let reverse_value = nominal_value(&reverse_order, name);

            match (&source_value.0, &reverse_value.0) {
                (
                    crate::types::dynamic::Union::Struct(source, ..),
                    crate::types::dynamic::Union::Struct(reverse, ..),
                ) => {
                    assert_eq!(source.identity(), reverse.identity());
                    assert_eq!(source.schema(), reverse.schema());
                }
                (
                    crate::types::dynamic::Union::Enum(source, ..),
                    crate::types::dynamic::Union::Enum(reverse, ..),
                ) => {
                    assert_eq!(source.identity(), reverse.identity());
                    assert_eq!(source.schema(), reverse.schema());
                }
                _ => panic!("expected matching nominal runtime forms"),
            }
        }

        let mut changed_item_ids = 0;
        for name in ["Envelope", "Message", "consume"] {
            let source_item = named_item(&source_order, name);
            let reverse_item = named_item(&reverse_order, name);

            assert_eq!(source_item.key, reverse_item.key);
            changed_item_ids += usize::from(source_item.id != reverse_item.id);
        }
        assert_eq!(changed_item_ids, 2);

        let Kind::Struct(envelope) = &source_order.items()[0].kind else {
            panic!("expected resolved struct");
        };
        let Type::Path(message) = &envelope.fields[0].ty else {
            panic!("expected resolved path");
        };
        let Kind::Struct(reverse_envelope) = &reverse_order.items()[0].kind else {
            panic!("expected resolved struct");
        };
        let Type::Path(reverse_message) = &reverse_envelope.fields[0].ty else {
            panic!("expected resolved path");
        };

        assert_ne!(message.target, reverse_message.target);
        assert_eq!(
            script_identity(&message.target),
            script_identity(&reverse_message.target)
        );
    }

    #[test]
    fn rename_move_and_package_changes_replace_stable_identity() {
        let engine = Engine::new();
        let host = host::Registry::new();
        let root = Namespace::new("counter", &[]);
        let moved = Namespace::new("counter", &["model"]);
        let other_package = Namespace::new("counter_components", &[]);
        let counter = resolve_with(
            engine.parse_unit("struct Counter { value: i64 }").unwrap(),
            &host,
            &root,
            AllocationOrder::Source,
        )
        .unwrap();
        let renamed = resolve_with(
            engine.parse_unit("struct Tally { value: i64 }").unwrap(),
            &host,
            &root,
            AllocationOrder::Source,
        )
        .unwrap();
        let moved = resolve_with(
            engine.parse_unit("struct Counter { value: i64 }").unwrap(),
            &host,
            &moved,
            AllocationOrder::Source,
        )
        .unwrap();
        let repackaged = resolve_with(
            engine.parse_unit("struct Counter { value: i64 }").unwrap(),
            &host,
            &other_package,
            AllocationOrder::Source,
        )
        .unwrap();

        let counter = definition(&counter, "Counter");
        let renamed = definition(&renamed, "Tally");
        let moved = definition(&moved, "Counter");
        let repackaged = definition(&repackaged, "Counter");

        assert_eq!(counter.key().to_string(), "counter::Counter");
        assert_eq!(moved.key().to_string(), "counter::model::Counter");
        assert_ne!(counter.key(), renamed.key());
        assert_ne!(counter.key(), moved.key());
        assert_ne!(counter.key(), repackaged.key());
        assert_ne!(counter.schema(), renamed.schema());
        assert_ne!(counter.schema(), moved.schema());
        assert_ne!(counter.schema(), repackaged.schema());
    }

    #[test]
    fn ordered_schema_shape_and_capabilities_change_fingerprint() {
        let cases = [
            "struct Shape { left: i64, right: bool }",
            "struct Shape { right: bool, left: i64 }",
            "struct Shape { first: i64, right: bool }",
            "struct Shape { left: bool, right: bool }",
            "#[derive(Clone)] struct Shape { left: i64, right: bool }",
            "#[derive(Clone, Copy)] struct Shape { left: i64, right: bool }",
        ];
        assert_distinct_schemas("Shape", &cases);

        let cases = [
            "enum Event { Ready, Changed(i64) }",
            "enum Event { Changed(i64), Ready }",
            "enum Event { Idle, Changed(i64) }",
            "enum Event { Ready, Changed(bool) }",
            "enum Event { Ready, Changed(i64, bool) }",
            "#[derive(Debug)] enum Event { Ready, Changed(i64) }",
        ];
        assert_distinct_schemas("Event", &cases);
    }

    #[test]
    fn method_keys_and_signatures_ignore_dense_type_allocation_order() {
        let engine = Engine::new();
        let host = host::Registry::new();
        let namespace = Namespace::new("counter", &["model"]);
        let source = r#"
            struct Counter { value: i64 }
            enum Message { Tick }
            impl Counter {
                fn update(&mut self, message: Message) {}
                fn value(&self) -> i64 { self.value }
            }
        "#;
        let source_order = resolve_with(
            engine.parse_unit(source).unwrap(),
            &host,
            &namespace,
            AllocationOrder::Source,
        )
        .unwrap();
        let reverse_order = resolve_with(
            engine.parse_unit(source).unwrap(),
            &host,
            &namespace,
            AllocationOrder::Reverse,
        )
        .unwrap();

        let stable = |program: &Program| {
            program
                .methods()
                .iter()
                .map(|method| {
                    (
                        method.key().clone(),
                        method.fingerprint().clone(),
                        method.owner_identity().clone(),
                        method.owner_schema().clone(),
                    )
                })
                .collect::<Vec<_>>()
        };

        assert_eq!(stable(&source_order), stable(&reverse_order));
        assert_eq!(source_order.method_count(), 2);
        assert_ne!(
            source_order.methods()[0].owner(),
            reverse_order.methods()[0].owner()
        );
    }

    #[test]
    fn executable_item_values_ignore_reversed_dense_allocation() {
        let engine = Engine::new();
        let host = host::Registry::new();
        let namespace = Namespace::new("counter", &["model"]);
        let source = r#"
            pub fn free(value: i64) -> i64 { value }
            struct Counter { value: i64 }
            enum Message { Tick, Add(i64) }
            impl Counter {
                fn reset() {}
                fn value(&self) -> i64 { self.value }
            }
        "#;
        let source_order = resolve_with(
            engine.parse_unit(source).unwrap(),
            &host,
            &namespace,
            AllocationOrder::Source,
        )
        .unwrap();
        let reverse_order = resolve_with(
            engine.parse_unit(source).unwrap(),
            &host,
            &namespace,
            AllocationOrder::Reverse,
        )
        .unwrap();
        let source_order = program::Executable::from_resolved(
            &engine,
            source_order,
            None,
            engine.optimization_level,
        )
        .unwrap();
        let reverse_order = program::Executable::from_resolved(
            &engine,
            reverse_order,
            None,
            engine.optimization_level,
        )
        .unwrap();

        let source_values: Vec<_> = source_order.items().cloned().collect();
        let reverse_values: Vec<_> = reverse_order.items().cloned().collect();

        assert_eq!(source_values, reverse_values);
        assert!(source_values.iter().any(|value| {
            value.kind() == crate::item::Kind::Function && value.key().name() == "free"
        }));
        assert!(source_values.iter().any(|value| {
            value.kind() == crate::item::Kind::AssociatedFunction && value.key().name() == "reset"
        }));
        assert!(source_values.iter().any(|value| {
            value.kind() == crate::item::Kind::Method && value.key().name() == "value"
        }));
        assert!(source_values.iter().any(|value| {
            value.kind() == crate::item::Kind::VariantConstructor && value.key().name() == "Add"
        }));
    }

    fn assert_distinct_schemas(name: &str, cases: &[&str]) {
        let engine = Engine::new();
        let host = host::Registry::new();
        let namespace = Namespace::new("schema_tests", &[]);
        let fingerprints: Vec<program::schema::Fingerprint> = cases
            .iter()
            .map(|source| {
                let program = resolve_with(
                    engine.parse_unit(source).unwrap(),
                    &host,
                    &namespace,
                    AllocationOrder::Source,
                )
                .unwrap();
                definition(&program, name).schema().clone()
            })
            .collect();

        for (index, fingerprint) in fingerprints.iter().enumerate() {
            assert!(
                fingerprints[index + 1..]
                    .iter()
                    .all(|other| other != fingerprint),
                "schema case {index} unexpectedly matched a later case"
            );
        }
    }

    fn definition<'a>(program: &'a Program, name: &str) -> &'a type_def::Definition {
        program
            .types()
            .iter()
            .find(|definition| definition.name() == name)
            .unwrap()
    }

    fn nominal_value(program: &Program, name: &str) -> crate::Dynamic {
        let definition = definition(program, name);
        let shell = named_item(program, name);

        match &shell.kind {
            Kind::Struct(item) => {
                let fields = item
                    .fields
                    .iter()
                    .map(|field| value_for_type(program, &field.ty))
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                let identity = type_def::Identity::Script(definition.key().clone());
                let model = program::model::Model::new(program::GenerationId::issue(), program);
                let proof = model
                    .prove_struct(&identity, definition.schema(), fields)
                    .unwrap();
                crate::types::nominal::Struct::construct(
                    &model,
                    &identity,
                    definition.schema(),
                    proof,
                )
                .unwrap()
                .into()
            }
            Kind::Enum(item) => {
                let variant = item
                    .variants
                    .iter()
                    .position(|variant| variant.payload.is_empty())
                    .expect("test enum has a zero-payload variant");
                let identity = type_def::Identity::Script(definition.key().clone());
                let variant =
                    crate::types::nominal::variant::Id::from_name(&item.variants[variant].name);
                let model = program::model::Model::new(program::GenerationId::issue(), program);
                let proof = model
                    .prove_enum(&identity, definition.schema(), &variant, Box::default())
                    .unwrap();
                crate::types::nominal::Enum::construct(
                    &model,
                    &identity,
                    definition.schema(),
                    &variant,
                    proof,
                )
                .unwrap()
                .into()
            }
            Kind::Use(_) | Kind::Impl(_) | Kind::Trait(_) | Kind::Function(_) => {
                panic!("expected nominal item")
            }
        }
    }

    fn value_for_type(program: &Program, ty: &Type) -> crate::Dynamic {
        match ty {
            Type::Unit(_) => crate::Dynamic::UNIT,
            Type::Tuple { .. } => panic!("test schemas contain no tuples"),
            Type::Reference { .. } => panic!("test schemas contain no references"),
            Type::Path(path) => match &path.target {
                Target::Builtin => match path.name.as_str() {
                    "bool" => false.into(),
                    "char" => '\0'.into(),
                    "i64" => 0_i64.into(),
                    "String" => crate::ImmutableString::new().into(),
                    "f64" => 0.0_f64.into(),
                    _ => panic!("unexpected builtin in test schema"),
                },
                Target::Parameter | Target::Projection(..) => {
                    panic!("test schemas contain no generic parameters")
                }
                Target::Script { identity, .. } => {
                    let type_def::Identity::Script(key) = identity else {
                        panic!("script target must carry script identity");
                    };
                    nominal_value(program, key.name())
                }
                Target::Host { .. } => panic!("test schemas contain no host values"),
                Target::Alias(_) => panic!("test schemas contain no aliases"),
            },
        }
    }

    fn named_item<'a>(program: &'a Program, name: &str) -> &'a super::item::Shell {
        program
            .items()
            .iter()
            .find(|item| item.key.as_ref().is_some_and(|key| key.name() == name))
            .unwrap()
    }

    fn script_identity(target: &Target) -> &type_def::Identity {
        let Target::Script { identity, .. } = target else {
            panic!("expected script target");
        };
        identity
    }

    fn path_identity(ty: &Type) -> &type_def::Identity {
        let Type::Path(path) = ty else {
            panic!("expected path type");
        };
        script_identity(&path.target)
    }

    struct Column;
}
