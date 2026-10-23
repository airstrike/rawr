//! Aggregate semantic resolution for a validated static package graph.

use std::collections::{BTreeMap, BTreeSet};

use super::{item, type_ref, Error, LocatedError, Namespace, Program, Script};
use crate::package::{graph, source, Graph};
use crate::{host, parse, program, type_def, ImmutableString, Span};

type UnitBindings = (
    Vec<Option<item::Use>>,
    BTreeMap<ImmutableString, type_ref::Target>,
    BTreeMap<ImmutableString, (source::Id, Span)>,
);

#[derive(Debug)]
pub(crate) struct Failure {
    pub(crate) primary: source::Id,
    pub(crate) secondary: Option<source::Id>,
    pub(crate) error: Error,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Address {
    package: ImmutableString,
    modules: Box<[ImmutableString]>,
    name: ImmutableString,
}

#[derive(Clone)]
struct Declaration {
    source: source::Id,
    span: Span,
    public: bool,
    target: Target,
}

#[derive(Clone)]
enum Target {
    Module,
    /// A crate trait: Rawr's traits are known by name in the whole crate,
    /// so importing one binds nothing new.
    Trait,
    HostModuleItem {
        definitions: Box<[host::item::Definition]>,
        owner: Option<host::facade::Dependency>,
    },
    Type(type_ref::Target),
    Item(crate::item::Key),
    HostItem {
        definitions: Box<[host::item::Definition]>,
        owner: Option<host::facade::Dependency>,
    },
    HostEndpoint {
        contract: host::endpoint::Contract,
        owner: host::facade::Dependency,
    },
}

#[derive(Default)]
struct ResolvedQualifiedCalls {
    functions: BTreeMap<(program::source::Id, Span), crate::item::Key>,
    owners: BTreeMap<(program::source::Id, Span), type_def::Identity>,
    rust: BTreeMap<(program::source::Id, Span), Box<[ImmutableString]>>,
}

struct QualifiedCall {
    path: Box<[ImmutableString]>,
    path_span: Span,
    owner_span: Span,
}

#[derive(Clone)]
struct Export {
    source: source::Id,
    context: Context,
    path: Box<[ImmutableString]>,
    span: Span,
}

#[derive(Clone)]
struct Context {
    source: source::Id,
    package: graph::Id,
    namespace: Namespace,
}

struct Unit {
    context: Context,
    parsed: parse::Unit,
    slots: Vec<Option<Slot>>,
}

#[derive(Clone)]
struct Slot {
    id: crate::item::Id,
    key: Option<crate::item::Key>,
}

struct Resolver<'a> {
    #[cfg(feature = "rust-import")]
    native: Option<std::sync::Arc<super::native::Capture>>,
    host: &'a host::Registry,
    package: ImmutableString,
    packages: Box<[graph::Package]>,
    evidence: Box<[crate::diagnostic::Source]>,
    units: Vec<Unit>,
    entry_source: source::Id,
    declarations: BTreeMap<Address, Declaration>,
    exports: BTreeMap<Address, Export>,
    /// Private `use` bindings, so a path may start with an imported module.
    aliases: BTreeMap<Address, Export>,
    type_declarations: Vec<type_def::Declaration>,
}

pub(crate) fn resolve(graph: Graph, host: &host::Registry) -> Result<Program, Failure> {
    Resolver::new(graph, host)?.resolve()
}

#[cfg(feature = "rust-import")]
pub(crate) fn resolve_captured(
    graph: Graph,
    host: &host::Registry,
    native: std::sync::Arc<super::native::Capture>,
) -> Result<Program, Failure> {
    let mut resolver = Resolver::new(graph, host)?;
    resolver.native = Some(native);
    resolver.resolve()
}

impl<'a> Resolver<'a> {
    fn new(graph: Graph, host: &'a host::Registry) -> Result<Self, Failure> {
        let (packages, sources, parsed, entry_package, entry_source) = graph.into_parts();
        let package = packages[entry_package.index()].name().into();
        let evidence = sources
            .iter()
            .map(|source| source.diagnostic().clone())
            .collect();
        let mut package_by_name = BTreeMap::new();
        for package in &packages {
            package_by_name.insert(package.name().to_owned(), package.id());
        }

        let units = parsed
            .into_vec()
            .into_iter()
            .map(|unit| {
                let source = &sources[unit.source.index()];
                let package = package_by_name[source.package()];
                Unit {
                    context: Context {
                        source: unit.source,
                        package,
                        namespace: Namespace {
                            package: source.package().into(),
                            modules: source.modules().into(),
                        },
                    },
                    parsed: unit.parsed,
                    slots: Vec::new(),
                }
            })
            .collect();

        let mut resolver = Self {
            #[cfg(feature = "rust-import")]
            native: None,
            host,
            package,
            packages,
            evidence,
            units,
            entry_source,
            declarations: BTreeMap::new(),
            exports: BTreeMap::new(),
            aliases: BTreeMap::new(),
            type_declarations: Vec::new(),
        };
        resolver.predeclare()?;
        Ok(resolver)
    }

    fn predeclare(&mut self) -> Result<(), Failure> {
        let mut next_item = 0_usize;

        for unit_index in 0..self.units.len() {
            let context = self.units[unit_index].context.clone();
            let items = self.units[unit_index].parsed.items().to_vec();
            let mut slots = Vec::with_capacity(items.len());

            for parsed in &items {
                if let crate::item::Parsed::Module(module) = parsed {
                    if let Some(preview) = module
                        .header
                        .attributes
                        .iter()
                        .find_map(crate::item::Attribute::preview)
                    {
                        return Err(Failure {
                            primary: context.source,
                            secondary: None,
                            error: Error::InvalidPreview {
                                preview: module.name.text.clone(),
                                reason: "`#[preview]` is supported only on functions and methods"
                                    .into(),
                                reference: preview.span,
                                declaration: Some(module.name.span),
                            },
                        });
                    }
                    let address = context.address(&module.name.text);
                    self.insert_declaration(
                        address.clone(),
                        Declaration {
                            source: context.source,
                            span: module.name.span,
                            public: module.header.visibility.is_some(),
                            target: Target::Module,
                        },
                    )?;
                    slots.push(None);
                    continue;
                }

                if let crate::item::Parsed::Trait(declared) = parsed {
                    self.insert_declaration(
                        context.address(&declared.name.text),
                        Declaration {
                            source: context.source,
                            span: declared.name.span,
                            public: declared.header.visibility.is_some(),
                            target: Target::Trait,
                        },
                    )?;
                }
                let id = crate::item::Id::from_index(next_item);
                next_item += 1;
                let mut slot = Slot { id, key: None };

                if let Some((name, type_kind, item_kind)) = super::named_declaration(parsed) {
                    let key = context.namespace.item_key(&name.text, item_kind);
                    slot.key = Some(key.clone());
                    let target = match type_kind {
                        Some(kind) => {
                            let definition = type_def::Id::from_index(self.type_declarations.len());
                            let type_key = context.namespace.type_key(&name.text);
                            self.type_declarations.push(type_def::Declaration::new(
                                definition,
                                id,
                                type_key.clone(),
                                name.text.clone(),
                                name.span,
                                kind,
                            ));
                            Target::Type(type_ref::Target::Script {
                                definition,
                                identity: type_def::Identity::Script(type_key),
                            })
                        }
                        None => Target::Item(key),
                    };
                    self.insert_declaration(
                        context.address(&name.text),
                        Declaration {
                            source: context.source,
                            span: name.span,
                            public: header(parsed)
                                .is_some_and(|header| header.visibility.is_some()),
                            target,
                        },
                    )?;
                }
                slots.push(Some(slot));
            }

            for parsed in &items {
                let crate::item::Parsed::Use(use_item) = parsed else {
                    continue;
                };
                if use_item.header.visibility.is_none() {
                    for (name, path) in use_paths(use_item) {
                        self.aliases.insert(
                            context.address(&name.text),
                            Export {
                                source: context.source,
                                context: context.clone(),
                                path,
                                span: name.span,
                            },
                        );
                    }
                    continue;
                }
                for (name, path) in use_paths(use_item) {
                    let address = context.address(&name.text);
                    if let Some(previous) = self.declarations.get(&address) {
                        return Err(Failure {
                            primary: context.source,
                            secondary: Some(previous.source),
                            error: Error::DuplicateItem {
                                name: name.text,
                                declaration: name.span,
                                previous_declaration: previous.span,
                            },
                        });
                    }
                    if let Some(previous) = self.exports.insert(
                        address,
                        Export {
                            source: context.source,
                            context: context.clone(),
                            path,
                            span: name.span,
                        },
                    ) {
                        return Err(Failure {
                            primary: context.source,
                            secondary: Some(previous.source),
                            error: Error::DuplicateItem {
                                name: name.text,
                                declaration: name.span,
                                previous_declaration: previous.span,
                            },
                        });
                    }
                }
            }

            self.units[unit_index].slots = slots;
        }
        Ok(())
    }

    fn insert_declaration(
        &mut self,
        address: Address,
        declaration: Declaration,
    ) -> Result<(), Failure> {
        if let Some(previous) = self
            .declarations
            .insert(address.clone(), declaration.clone())
        {
            return Err(Failure {
                primary: declaration.source,
                secondary: Some(previous.source),
                error: Error::DuplicateItem {
                    name: address.name,
                    declaration: declaration.span,
                    previous_declaration: previous.span,
                },
            });
        }
        Ok(())
    }

    fn resolve_preview_paths(
        &self,
    ) -> Result<BTreeMap<(program::source::Id, Span), crate::preview::PathTarget>, Failure> {
        let mut resolved = BTreeMap::new();
        for unit in &self.units {
            for parsed in unit.parsed.items() {
                for syntax in preview_syntaxes(parsed) {
                    for path in syntax
                        .seed
                        .iter()
                        .chain(syntax.update.iter())
                        .chain(syntax.themes.iter())
                    {
                        let segments = path_segments(path);
                        if !self.is_script_path(&unit.context, &segments) {
                            continue;
                        }
                        let target =
                            self.resolve_preview_path(&unit.context, &segments, path.span)?;
                        resolved.insert(
                            (
                                program::source::Id::from_index(unit.context.source.index()),
                                path.span,
                            ),
                            target,
                        );
                    }
                }
            }
        }
        Ok(resolved)
    }

    fn resolve_qualified_calls(&self) -> Result<ResolvedQualifiedCalls, Failure> {
        let mut resolved = ResolvedQualifiedCalls::default();
        for unit in &self.units {
            for parsed in unit.parsed.items() {
                for call in qualified_calls(parsed) {
                    if !self.is_script_path(&unit.context, &call.path) {
                        continue;
                    }
                    let source = program::source::Id::from_index(unit.context.source.index());
                    // `crate::model::Thing::new` into a `.rs` module
                    // is the app crate's `polyglot::model::Thing::new`.
                    if let Some(spelled) = self.rust_spelling(&unit.context, &call.path) {
                        resolved
                            .rust
                            .insert((source, call.path_span), spelled.into_boxed_slice());
                        continue;
                    }

                    if call.path.len() >= 3 {
                        let mut active = BTreeSet::new();
                        match self.resolve_path(
                            &unit.context,
                            &call.path[..call.path.len() - 1],
                            call.owner_span,
                            &mut active,
                        )? {
                            Target::Type(type_ref::Target::Script { identity, .. }) => {
                                resolved.owners.insert((source, call.owner_span), identity);
                                continue;
                            }
                            Target::Module
                            | Target::Trait
                            | Target::HostModuleItem { .. }
                            | Target::Item(_) => {}
                            Target::HostItem { .. }
                            | Target::HostEndpoint { .. }
                            | Target::Type(
                                type_ref::Target::Builtin
                                | type_ref::Target::Host { .. }
                                | type_ref::Target::Parameter
                                | type_ref::Target::Projection(..),
                            ) => continue,
                        }
                    }

                    let mut active = BTreeSet::new();
                    match self.resolve_path(
                        &unit.context,
                        &call.path,
                        call.path_span,
                        &mut active,
                    )? {
                        Target::Item(key) => {
                            resolved.functions.insert((source, call.path_span), key);
                        }
                        // A qualified type, as in a `let` annotation.
                        Target::Type(type_ref::Target::Script { identity, .. }) => {
                            resolved.owners.insert((source, call.path_span), identity);
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(resolved)
    }

    fn resolve_qualified_pattern_owners(
        &self,
    ) -> Result<BTreeMap<(program::source::Id, Span), type_def::Identity>, Failure> {
        let mut resolved = BTreeMap::new();
        for unit in &self.units {
            for parsed in unit.parsed.items() {
                for (path, owner) in qualified_pattern_owners(parsed) {
                    if !self.is_script_path(&unit.context, &path) {
                        continue;
                    }
                    let mut active = BTreeSet::new();
                    match self.resolve_path(&unit.context, &path, owner, &mut active)? {
                        Target::Type(type_ref::Target::Script { identity, .. }) => {
                            resolved.insert(
                                (
                                    program::source::Id::from_index(unit.context.source.index()),
                                    owner,
                                ),
                                identity,
                            );
                        }
                        Target::Module
                        | Target::Trait
                        | Target::HostModuleItem { .. }
                        | Target::Item(_)
                        | Target::HostItem { .. }
                        | Target::HostEndpoint { .. }
                        | Target::Type(
                            type_ref::Target::Builtin
                            | type_ref::Target::Host { .. }
                            | type_ref::Target::Parameter
                            | type_ref::Target::Projection(..),
                        ) => {}
                    }
                }
            }
        }
        Ok(resolved)
    }

    fn resolve_preview_path(
        &self,
        context: &Context,
        segments: &[ImmutableString],
        reference: Span,
    ) -> Result<crate::preview::PathTarget, Failure> {
        if segments.len() >= 3 {
            let mut active = BTreeSet::new();
            match self.resolve_path(
                context,
                &segments[..segments.len() - 1],
                reference,
                &mut active,
            )? {
                Target::Type(type_ref::Target::Script { identity, .. }) => {
                    return Ok(crate::preview::PathTarget::Associated {
                        owner: identity,
                        member: segments
                            .last()
                            .expect("qualified preview paths have a member")
                            .clone(),
                    });
                }
                Target::Module | Target::Trait => {}
                Target::Type(
                    type_ref::Target::Builtin
                    | type_ref::Target::Host { .. }
                    | type_ref::Target::Parameter
                    | type_ref::Target::Projection(..),
                )
                | Target::Item(_)
                | Target::HostItem { .. }
                | Target::HostEndpoint { .. }
                | Target::HostModuleItem { .. } => {
                    return Err(Failure {
                        primary: context.source,
                        secondary: None,
                        error: Error::InvalidPreview {
                            preview: segments.join("::").into(),
                            reason:
                                "qualified preview callback prefix is not a script module or type"
                                    .into(),
                            reference,
                            declaration: None,
                        },
                    });
                }
            }
        }

        let mut active = BTreeSet::new();
        match self.resolve_path(context, segments, reference, &mut active)? {
            Target::Item(key) => Ok(crate::preview::PathTarget::Item(key)),
            Target::Module
            | Target::Trait
            | Target::Type(_)
            | Target::HostItem { .. }
            | Target::HostEndpoint { .. }
            | Target::HostModuleItem { .. } => Err(Failure {
                primary: context.source,
                secondary: None,
                error: Error::InvalidPreview {
                    preview: segments.join("::").into(),
                    reason:
                        "preview callback path must resolve to a script function or associated item"
                            .into(),
                    reference,
                    declaration: None,
                },
            }),
        }
    }

    fn resolve(mut self) -> Result<Program, Failure> {
        self.validate_exports()?;
        let preview_paths = self.resolve_preview_paths()?;
        let qualified_calls = self.resolve_qualified_calls()?;
        let qualified_pattern_owners = self.resolve_qualified_pattern_owners()?;
        let mut shells = Vec::new();
        let mut scripts = Vec::new();

        let units = std::mem::take(&mut self.units);
        for Unit {
            context,
            parsed,
            slots,
        } in units
        {
            let (source_map, parsed_items, legacy_script, legacy_source) = parsed.into_parts();
            #[cfg(feature = "no_function")]
            drop(legacy_source);

            if context.source != self.entry_source
                && legacy_script
                    .as_ref()
                    .is_some_and(|script| !script.statements().is_empty())
            {
                let position = legacy_script
                    .as_ref()
                    .and_then(|script| {
                        script
                            .statements()
                            .iter()
                            .map(crate::ast::Stmt::position)
                            .find(|position| !position.is_none())
                    })
                    .unwrap_or(crate::Position::START);
                return Err(Failure {
                    primary: context.source,
                    secondary: None,
                    error: Error::UnsupportedNominalSyntax {
                        construct: "top-level executable statements outside the application entry"
                            .into(),
                        source: Span::new(position, position),
                    },
                });
            }

            let (uses, types, _occupied) = self.resolve_unit_bindings(&context, &parsed_items)?;
            for (index, parsed) in parsed_items.into_vec().into_iter().enumerate() {
                let Some(slot) = &slots[index] else {
                    continue;
                };
                let shell = super::resolve_item(
                    slot.id,
                    program::source::Id::from_index(context.source.index()),
                    slot.key.clone(),
                    uses[index].clone(),
                    parsed,
                    &types,
                    self.host,
                    #[cfg(feature = "rust-import")]
                    self.native.as_deref().map(|capture| {
                        capture
                            .at(program::source::Id::from_index(context.source.index()))
                            .with_names(&_occupied)
                    }),
                )
                .map_err(|error| Failure {
                    primary: context.source,
                    secondary: None,
                    error,
                })?;
                shells.push(shell);
            }
            scripts.push((
                context.source,
                source_map,
                Script {
                    source: program::source::Id::from_index(context.source.index()),
                    ast: legacy_script,
                    #[cfg(not(feature = "no_function"))]
                    text: legacy_source,
                },
            ));
        }

        let definitions =
            finish_definitions(self.type_declarations, &shells).map_err(Failure::from_located)?;
        let (methods, builtin_methods) =
            super::resolve_methods(&mut shells, &definitions, self.host)
                .map_err(Failure::from_located)?;
        super::resolve_functions(&mut shells).map_err(Failure::from_located)?;
        let application = super::application::resolve(
            &mut shells,
            &methods,
            self.host,
            Some(program::source::Id::from_index(self.entry_source.index())),
        )
        .map_err(|error| Failure {
            primary: self.entry_source,
            secondary: None,
            error: Error::from(error),
        })?;

        scripts.sort_by_key(|(source, ..)| source.index());
        let source_maps = scripts
            .iter()
            .map(|(_, map, _)| map.clone())
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let scripts = scripts
            .into_iter()
            .map(|(_, _, script)| script)
            .collect::<Vec<_>>()
            .into_boxed_slice();

        let mut program = Program {
            #[cfg(feature = "rust-import")]
            native: self.native,
            package: self.package,
            source: program::Source::from_graph(
                program::source::Id::from_index(self.entry_source.index()),
                source_maps,
                self.evidence,
            ),
            items: shells.into_boxed_slice(),
            types: definitions.into_boxed_slice(),
            methods: methods.into_boxed_slice(),
            builtin_methods: builtin_methods.into_boxed_slice(),
            application,
            previews: Box::new([]),
            widgets: Box::new([]),
            preview_paths,
            qualified_functions: qualified_calls.functions,
            qualified_owners: qualified_calls.owners,
            qualified_pattern_owners,
            qualified_rust: qualified_calls.rust,
            scripts,
        };
        super::validate_invocation_storage(&program).map_err(Failure::from_located)?;
        program.widgets =
            crate::widget::resolve(&program, self.host).map_err(Failure::from_located)?;
        program.previews =
            crate::preview::resolve(&program, self.host).map_err(Failure::from_located)?;
        Ok(program)
    }

    fn validate_exports(&self) -> Result<(), Failure> {
        for (address, export) in &self.exports {
            let mut active = BTreeSet::new();
            active.insert(address.clone());
            let target =
                self.resolve_any_path(&export.context, &export.path, export.span, &mut active)?;
            if matches!(target, Target::Module | Target::HostModuleItem { .. }) {
                return Err(Failure {
                    primary: export.source,
                    secondary: None,
                    error: Error::InvalidImport {
                        path: export.path.join("::").into(),
                        reason:
                            "module re-exports are not supported; export the module with `pub mod`"
                                .into(),
                        reference: export.span,
                        declaration: None,
                    },
                });
            }
        }
        Ok(())
    }

    /// The names a glob import (`use dep::module::*;`) binds: every public
    /// name of that Rust module that no declaration or explicit import of
    /// this unit already binds.
    fn glob_paths(
        &self,
        context: &Context,
        parsed: &crate::item::Use,
        occupied: &BTreeMap<ImmutableString, (source::Id, Span)>,
    ) -> Result<Vec<(crate::item::Name, Box<[ImmutableString]>)>, Failure> {
        let crate::item::UseTree::Glob { prefix, span } = &parsed.tree else {
            return Ok(Vec::new());
        };
        let segments = path_segments(prefix);
        let module: ImmutableString = segments.join("::").into();
        // `use super::*;`, `use crate::shapes::*;`: every item that module
        // of this crate declares.
        if let Some(modules) = self.local_glob_module(context, &segments) {
            let mut names = self
                .declarations
                .keys()
                .filter(|address| {
                    address.package == context.namespace.package && *address.modules == *modules
                })
                .map(|address| address.name.clone())
                .filter(|name| !occupied.contains_key(name))
                .collect::<Vec<_>>();
            names.dedup();
            return Ok(names
                .into_iter()
                .map(|name| {
                    let mut path = segments.to_vec();
                    path.push(name.clone());
                    (
                        crate::item::Name {
                            span: *span,
                            text: name,
                        },
                        path.into_boxed_slice(),
                    )
                })
                .collect());
        }
        if parsed.header.visibility.is_some() {
            return Err(Failure {
                primary: context.source,
                secondary: None,
                error: Error::InvalidImport {
                    path: module,
                    reason: "glob re-exports are not supported".into(),
                    reference: *span,
                    declaration: None,
                },
            });
        }
        if !matches!(
            self.host.entry(&module),
            Some(host::Entry::Module | host::Entry::ModuleItem(_))
        ) {
            return Err(Failure {
                primary: context.source,
                secondary: None,
                error: Error::UnknownImport {
                    path: module,
                    reference: *span,
                },
            });
        }
        Ok(self
            .host
            .children(&module)
            .into_iter()
            .filter(|name| !occupied.contains_key(name))
            .map(|name| {
                let mut path = segments.to_vec();
                path.push(name.clone());
                (
                    crate::item::Name {
                        span: *span,
                        text: name,
                    },
                    path.into_boxed_slice(),
                )
            })
            .collect())
    }

    /// The modules of this crate a glob import's prefix names: `super`,
    /// `super::super`, `crate`, `crate::a`, `self::a` or a child module.
    fn local_glob_module(
        &self,
        context: &Context,
        segments: &[ImmutableString],
    ) -> Option<Vec<ImmutableString>> {
        let first = segments.first()?;
        let modules = context.namespace.modules.as_ref();
        let supers = segments
            .iter()
            .take_while(|segment| *segment == "super")
            .count();
        let mut absolute = Vec::new();
        if supers > 0 {
            absolute.extend_from_slice(&modules[..modules.len().checked_sub(supers)?]);
            absolute.extend_from_slice(&segments[supers..]);
        } else if first == "crate" {
            absolute.extend_from_slice(&segments[1..]);
        } else if first == "self" {
            absolute.extend_from_slice(modules);
            absolute.extend_from_slice(&segments[1..]);
        } else if self.is_child_module(context, first) {
            absolute.extend_from_slice(modules);
            absolute.extend_from_slice(segments);
        } else {
            return None;
        }
        Some(absolute)
    }

    fn resolve_unit_bindings(
        &self,
        context: &Context,
        parsed_items: &[crate::item::Parsed],
    ) -> Result<UnitBindings, Failure> {
        let mut occupied = BTreeMap::<ImmutableString, (source::Id, Span)>::new();
        let mut types = BTreeMap::new();

        // A module is in Rust's type namespace: a function or value of its
        // name may be imported beside it (`mod circle;` and
        // `use circle::circle;`).
        let mut modules = BTreeMap::<ImmutableString, (source::Id, Span)>::new();
        for (address, declaration) in &self.declarations {
            if address.package == context.namespace.package
                && address.modules == context.namespace.modules
            {
                if matches!(declaration.target, Target::Module) {
                    modules.insert(address.name.clone(), (declaration.source, declaration.span));
                    continue;
                }
                occupied.insert(address.name.clone(), (declaration.source, declaration.span));
                if let Target::Type(target) = &declaration.target {
                    types.insert(address.name.clone(), target.clone());
                }
            }
        }

        self.resolve_qualified_header_types(context, parsed_items, &mut types)?;
        let mut uses = vec![None; parsed_items.len()];

        // Explicit imports first: a name they or a declaration bind shadows
        // the same name from a glob import, as in Rust.
        let glob = |parsed: &crate::item::Parsed| {
            matches!(parsed, crate::item::Parsed::Use(used)
                if matches!(used.tree, crate::item::UseTree::Glob { .. }))
        };
        let mut order = (0..parsed_items.len()).collect::<Vec<_>>();
        order.sort_by_key(|index| glob(&parsed_items[*index]));
        for index in order {
            let crate::item::Parsed::Use(parsed) = &parsed_items[index] else {
                continue;
            };
            let mut bindings = Vec::new();

            let globbed = self.glob_paths(context, parsed, &occupied)?;
            for (name, path) in use_paths(parsed).into_iter().chain(globbed) {
                // `use keyboard::key;` after `use iced::keyboard;`: a path
                // through a library module this unit imported, as Rust's
                // paths start at a name in scope.
                let path = match path.split_first() {
                    Some((first, rest)) if !rest.is_empty() => uses
                        .iter()
                        .flatten()
                        .flat_map(|used: &item::Use| used.bindings.iter())
                        .find(|binding| {
                            binding.name.text == *first
                                && matches!(
                                    binding.target,
                                    item::Import::Module | item::Import::ModuleItem { .. }
                                )
                                && self.host.entry(&binding.qualified).is_some()
                        })
                        .map_or(path.clone(), |binding| {
                            binding
                                .qualified
                                .split("::")
                                .map(ImmutableString::from)
                                .chain(rest.iter().cloned())
                                .collect()
                        }),
                    _ => path,
                };
                let mut active = BTreeSet::new();
                let target = self.resolve_any_path(context, &path, name.span, &mut active)?;
                // An item of a `.rs` module keeps its crate spelling.
                let qualified: ImmutableString = self
                    .rust_spelling(context, &path)
                    .unwrap_or_else(|| path.to_vec())
                    .join("::")
                    .into();
                let import = match target {
                    Target::Module | Target::Trait => item::Import::Module,
                    Target::HostModuleItem { definitions, owner } => {
                        item::Import::ModuleItem { definitions, owner }
                    }
                    Target::Type(type_ref::Target::Script {
                        definition,
                        identity,
                    }) => {
                        types.insert(
                            name.text.clone(),
                            type_ref::Target::Script {
                                definition,
                                identity: identity.clone(),
                            },
                        );
                        item::Import::ScriptType {
                            definition,
                            identity,
                        }
                    }
                    Target::Type(type_ref::Target::Host { definition, owner }) => {
                        types.insert(
                            name.text.clone(),
                            type_ref::Target::Host {
                                definition: definition.clone(),
                                owner: owner.clone(),
                            },
                        );
                        item::Import::Type { definition, owner }
                    }
                    Target::Type(
                        type_ref::Target::Builtin
                        | type_ref::Target::Parameter
                        | type_ref::Target::Projection(..),
                    ) => {
                        return Err(Failure {
                            primary: context.source,
                            secondary: None,
                            error: Error::InvalidImport {
                                path: qualified,
                                reason: "builtin types do not require `use`".into(),
                                reference: name.span,
                                declaration: None,
                            },
                        });
                    }
                    Target::Item(key) => item::Import::ScriptItem { key },
                    Target::HostItem { definitions, owner } => {
                        item::Import::Item { definitions, owner }
                    }
                    Target::HostEndpoint { contract, owner } => {
                        item::Import::Endpoint { contract, owner }
                    }
                };
                let value = matches!(
                    import,
                    item::Import::ScriptItem { .. }
                        | item::Import::Item { .. }
                        | item::Import::Endpoint { .. }
                );
                let previous = if value {
                    occupied.insert(name.text.clone(), (context.source, name.span))
                } else {
                    occupied
                        .insert(name.text.clone(), (context.source, name.span))
                        .or_else(|| modules.get(&name.text).copied())
                };
                if let Some((previous_source, previous_span)) = previous {
                    return Err(Failure {
                        primary: context.source,
                        secondary: Some(previous_source),
                        error: Error::DuplicateItem {
                            name: name.text,
                            declaration: name.span,
                            previous_declaration: previous_span,
                        },
                    });
                }
                bindings.push(item::Binding {
                    name,
                    qualified,
                    target: import,
                });
            }

            uses[index] = Some(item::Use {
                header: parsed.header.clone(),
                bindings: bindings.into_boxed_slice(),
                span: parsed.span,
            });
        }
        super::module_header_types(parsed_items, &uses, self.host, &mut types);
        for (name, declaration) in modules {
            occupied.entry(name).or_insert(declaration);
        }
        Ok((uses, types, occupied))
    }

    fn resolve_qualified_header_types(
        &self,
        context: &Context,
        parsed_items: &[crate::item::Parsed],
        types: &mut BTreeMap<ImmutableString, type_ref::Target>,
    ) -> Result<(), Failure> {
        let mut paths = Vec::new();
        for item in parsed_items {
            collect_item_type_paths(item, &mut paths);
        }
        for path in paths {
            let segments = path_segments(path);
            if segments.len() < 2 || !self.is_script_path(context, &segments) {
                continue;
            }
            let mut active = BTreeSet::new();
            match self.resolve_path(context, &segments, path.span, &mut active)? {
                Target::Type(target) => {
                    types.insert(segments.join("::").into(), target);
                }
                Target::Module
                | Target::Trait
                | Target::HostModuleItem { .. }
                | Target::Item(_)
                | Target::HostItem { .. } => {}
                Target::HostEndpoint { .. } => {}
            }
        }
        Ok(())
    }

    fn is_script_path(&self, context: &Context, path: &[ImmutableString]) -> bool {
        self.is_direct_script_path(context, path)
            || path.len() > 1
                && path
                    .first()
                    .is_some_and(|first| self.module_alias(context, first).is_some())
    }

    fn is_direct_script_path(&self, context: &Context, path: &[ImmutableString]) -> bool {
        path.first().is_some_and(|first| {
            first == "crate" || first == "super" || self.is_child_module(context, first)
        })
    }

    /// A `use` binding of a script module in `context` (`use super::config;`),
    /// which a longer path may start with.
    fn module_alias(&self, context: &Context, name: &str) -> Option<&Export> {
        let address = context.address(name);
        let alias = self
            .aliases
            .get(&address)
            .or_else(|| self.exports.get(&address))?;
        let module = self.is_direct_script_path(&alias.context, &alias.path)
            && matches!(
                self.resolve_path(
                    &alias.context,
                    &alias.path,
                    alias.span,
                    &mut BTreeSet::new()
                ),
                Ok(Target::Module)
            );
        module.then_some(alias)
    }

    fn is_child_module(&self, context: &Context, name: &str) -> bool {
        self.declarations
            .get(&context.address(name))
            .is_some_and(|declaration| matches!(declaration.target, Target::Module))
    }

    fn resolve_any_path(
        &self,
        context: &Context,
        path: &[ImmutableString],
        reference: Span,
        active: &mut BTreeSet<Address>,
    ) -> Result<Target, Failure> {
        if self.is_script_path(context, path) {
            self.resolve_path(context, path, reference, active)
        } else {
            host_target(
                self.host,
                path,
                reference,
                #[cfg(feature = "rust-import")]
                self.native.as_deref().map(|capture| {
                    capture.at(program::source::Id::from_index(context.source.index()))
                }),
            )
            .map_err(|error| Failure {
                primary: context.source,
                secondary: None,
                error,
            })
        }
    }

    /// A path into a `.rs` module of a polyglot package
    /// (`crate::model::Thing`) names the app's own crate, imported under its
    /// crate name (`polyglot::model::Thing`).
    fn rust_module_item(
        &self,
        context: &Context,
        package: graph::Id,
        modules: &[ImmutableString],
        name: &ImmutableString,
        reference: Span,
    ) -> Option<Result<Target, Failure>> {
        let package = &self.packages[package.index()];
        let alias = package.rust_crate()?;
        if !package
            .rust_modules()
            .iter()
            .any(|module| modules.starts_with(module.modules()))
        {
            return None;
        }
        let mut path = Vec::with_capacity(modules.len() + 2);
        path.push(ImmutableString::from(alias));
        path.extend(modules.iter().cloned());
        path.push(name.clone());
        Some(
            host_target(
                self.host,
                &path,
                reference,
                #[cfg(feature = "rust-import")]
                self.native.as_deref().map(|capture| {
                    capture.at(program::source::Id::from_index(context.source.index()))
                }),
            )
            .map_err(|error| Failure {
                primary: context.source,
                secondary: None,
                error,
            }),
        )
    }

    /// `crate::…`, `super::…` or child-module paths that land in a `.rs`
    /// module of the current package, spelled through the app's crate name.
    fn rust_spelling(
        &self,
        context: &Context,
        path: &[ImmutableString],
    ) -> Option<Vec<ImmutableString>> {
        let package = &self.packages[context.package.index()];
        let alias = package.rust_crate()?;
        let first = path.first()?;
        let supers = path
            .iter()
            .take_while(|segment| *segment == "super")
            .count();
        let modules = context.namespace.modules.as_ref();
        let mut absolute = Vec::with_capacity(path.len() + modules.len());
        if first == "crate" {
            absolute.extend_from_slice(&path[1..]);
        } else if supers > 0 {
            absolute.extend_from_slice(&modules[..modules.len().checked_sub(supers)?]);
            absolute.extend_from_slice(&path[supers..]);
        } else if self.is_child_module(context, first) {
            absolute.extend_from_slice(modules);
            absolute.extend_from_slice(path);
        } else {
            return None;
        }
        let inside = package.rust_modules().iter().any(|module| {
            absolute.len() > module.modules().len() && absolute.starts_with(module.modules())
        });
        inside.then(|| {
            std::iter::once(ImmutableString::from(alias))
                .chain(absolute)
                .collect()
        })
    }

    fn resolve_path(
        &self,
        context: &Context,
        path: &[ImmutableString],
        reference: Span,
        active: &mut BTreeSet<Address>,
    ) -> Result<Target, Failure> {
        let Some(first) = path.first() else {
            return Err(self.unknown_import(context, path, reference));
        };
        let supers = path
            .iter()
            .take_while(|segment| *segment == "super")
            .count();
        let modules = context.namespace.modules.as_ref();
        let (package, base_modules, tail) = if supers > 0 {
            let Some(parent) = modules.len().checked_sub(supers) else {
                return Err(Failure {
                    primary: context.source,
                    secondary: None,
                    error: Error::InvalidImport {
                        path: path.join("::").into(),
                        reason: "too many leading `super` keywords".into(),
                        reference,
                        declaration: None,
                    },
                });
            };
            (context.package, &modules[..parent], &path[supers..])
        } else if first == "crate" {
            (context.package, &[][..], &path[1..])
        } else if self.is_child_module(context, first) {
            (context.package, context.namespace.modules.as_ref(), path)
        } else if let Some(alias) = self.module_alias(context, first).filter(|_| path.len() > 1) {
            // `use super::config;` then `config::GAP`: expand the binding,
            // whose own path starts directly at a module.
            let expanded = alias
                .path
                .iter()
                .chain(&path[1..])
                .cloned()
                .collect::<Vec<_>>();
            return self.resolve_path(&alias.context, &expanded, reference, active);
        } else {
            return Err(self.unknown_import(context, path, reference));
        };
        let Some((name, tail_modules)) = tail.split_last() else {
            return Err(self.unknown_import(context, path, reference));
        };
        let mut modules = Vec::with_capacity(base_modules.len() + tail_modules.len());
        modules.extend_from_slice(base_modules);
        modules.extend_from_slice(tail_modules);
        if let Some(target) = self.rust_module_item(context, package, &modules, name, reference) {
            return target;
        }
        let package_name: ImmutableString = self.packages[package.index()].name().into();
        let address = Address {
            package: package_name,
            modules: modules.into_boxed_slice(),
            name: name.clone(),
        };

        if let Some(declaration) = self.declarations.get(&address) {
            self.validate_visibility(context, &address, declaration, reference)?;
            return Ok(declaration.target.clone());
        }
        if let Some(export) = self.exports.get(&address) {
            self.validate_module_hops(context, &address, reference)?;
            if !active.insert(address.clone()) {
                return Err(Failure {
                    primary: context.source,
                    secondary: Some(export.source),
                    error: Error::InvalidImport {
                        path: path.join("::").into(),
                        reason: "cyclic public re-export".into(),
                        reference,
                        declaration: None,
                    },
                });
            }
            let target =
                self.resolve_any_path(&export.context, &export.path, export.span, active)?;
            active.remove(&address);
            return Ok(target);
        }
        Err(self.unknown_import(context, path, reference))
    }

    fn validate_visibility(
        &self,
        context: &Context,
        address: &Address,
        declaration: &Declaration,
        reference: Span,
    ) -> Result<(), Failure> {
        self.validate_module_hops(context, address, reference)?;
        if !declaration.public && !within(context, address, address.modules.len()) {
            return Err(Failure {
                primary: context.source,
                secondary: Some(declaration.source),
                error: Error::InvalidImport {
                    path: address.display(),
                    reason: "import targets a private item".into(),
                    reference,
                    declaration: Some(declaration.span),
                },
            });
        }
        Ok(())
    }

    fn validate_module_hops(
        &self,
        context: &Context,
        address: &Address,
        reference: Span,
    ) -> Result<(), Failure> {
        for depth in 0..address.modules.len() {
            let module = Address {
                package: address.package.clone(),
                modules: address.modules[..depth].into(),
                name: address.modules[depth].clone(),
            };
            let Some(module) = self.declarations.get(&module) else {
                return Err(self.unknown_import(context, &[], reference));
            };
            if !module.public && !within(context, address, depth) {
                return Err(Failure {
                    primary: context.source,
                    secondary: Some(module.source),
                    error: Error::InvalidImport {
                        path: address.display(),
                        reason: "import crosses a private module".into(),
                        reference,
                        declaration: Some(module.span),
                    },
                });
            }
        }
        Ok(())
    }

    fn unknown_import(
        &self,
        context: &Context,
        path: &[ImmutableString],
        reference: Span,
    ) -> Failure {
        Failure {
            primary: context.source,
            secondary: None,
            error: Error::UnknownImport {
                path: path.join("::").into(),
                reference,
            },
        }
    }
}

fn preview_syntaxes(parsed: &crate::item::Parsed) -> Vec<&crate::preview::Syntax> {
    fn collect<'a>(
        header: &'a crate::item::Header,
        syntaxes: &mut Vec<&'a crate::preview::Syntax>,
    ) {
        syntaxes.extend(
            header
                .attributes
                .iter()
                .filter_map(crate::item::Attribute::preview),
        );
    }

    let mut syntaxes = Vec::new();
    match parsed {
        crate::item::Parsed::Function(function) => collect(&function.header, &mut syntaxes),
        crate::item::Parsed::Impl(crate::item::Impl { functions, .. })
        | crate::item::Parsed::Trait(crate::item::Trait { functions, .. }) => {
            for function in functions {
                collect(&function.header, &mut syntaxes);
            }
        }
        crate::item::Parsed::Module(_)
        | crate::item::Parsed::Use(_)
        | crate::item::Parsed::Struct(_)
        | crate::item::Parsed::Enum(_)
        | crate::item::Parsed::LegacyFunction(_) => {}
    }
    syntaxes
}

fn qualified_calls(parsed: &crate::item::Parsed) -> Vec<QualifiedCall> {
    fn collect(syntax: &crate::item::BodySyntax, output: &mut Vec<QualifiedCall>) {
        let tokens = syntax.tokens();
        let mut index = 0;

        while index < tokens.len() {
            let crate::item::BodyTokenKind::Identifier(first) = tokens[index].kind() else {
                index += 1;
                continue;
            };
            if index > 0
                && matches!(
                    tokens[index - 1].kind(),
                    crate::item::BodyTokenKind::DoubleColon
                )
            {
                index += 1;
                continue;
            }

            let mut segments = vec![(first.clone(), tokens[index].span())];
            let mut end = index;
            while end + 2 < tokens.len()
                && matches!(
                    tokens[end + 1].kind(),
                    crate::item::BodyTokenKind::DoubleColon
                )
            {
                let crate::item::BodyTokenKind::Identifier(name) = tokens[end + 2].kind() else {
                    break;
                };
                segments.push((name.clone(), tokens[end + 2].span()));
                end += 2;
            }

            if segments.len() >= 2 {
                let owner_end = segments[segments.len() - 2].1.end();
                let path_end = segments[segments.len() - 1].1.end();
                output.push(QualifiedCall {
                    path: segments
                        .iter()
                        .map(|(name, _)| name.clone())
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                    path_span: Span::new(segments[0].1.start(), path_end),
                    owner_span: Span::new(segments[0].1.start(), owner_end),
                });
            }
            index = end.saturating_add(1);
        }
    }

    let mut output = Vec::new();
    match parsed {
        crate::item::Parsed::Function(function) => collect(&function.body_syntax, &mut output),
        crate::item::Parsed::Impl(crate::item::Impl { functions, .. })
        | crate::item::Parsed::Trait(crate::item::Trait { functions, .. }) => {
            for function in functions {
                collect(&function.body_syntax, &mut output);
            }
        }
        crate::item::Parsed::LegacyFunction(function) => {
            collect(&function.body_syntax, &mut output);
        }
        crate::item::Parsed::Module(_)
        | crate::item::Parsed::Use(_)
        | crate::item::Parsed::Struct(_)
        | crate::item::Parsed::Enum(_) => {}
    }
    output
}

fn qualified_pattern_owners(parsed: &crate::item::Parsed) -> Vec<(Box<[ImmutableString]>, Span)> {
    fn collect_paths(
        tokens: &[crate::item::BodyToken],
        output: &mut Vec<(Box<[ImmutableString]>, Span)>,
    ) {
        let mut index = 0;
        while index < tokens.len() {
            let crate::item::BodyTokenKind::Identifier(first) = tokens[index].kind() else {
                index += 1;
                continue;
            };
            if index > 0
                && matches!(
                    tokens[index - 1].kind(),
                    crate::item::BodyTokenKind::DoubleColon
                )
            {
                index += 1;
                continue;
            }

            let mut segments = vec![(first.clone(), tokens[index].span())];
            let mut end = index;
            while end + 2 < tokens.len()
                && matches!(
                    tokens[end + 1].kind(),
                    crate::item::BodyTokenKind::DoubleColon
                )
            {
                let crate::item::BodyTokenKind::Identifier(name) = tokens[end + 2].kind() else {
                    break;
                };
                segments.push((name.clone(), tokens[end + 2].span()));
                end += 2;
            }

            if segments.len() >= 3 {
                let owner_end = segments[segments.len() - 2].1.end();
                output.push((
                    segments[..segments.len() - 1]
                        .iter()
                        .map(|(name, _)| name.clone())
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                    Span::new(segments[0].1.start(), owner_end),
                ));
            }
            index = end.saturating_add(1);
        }
    }

    fn collect(syntax: &crate::item::BodySyntax, output: &mut Vec<(Box<[ImmutableString]>, Span)>) {
        use crate::item::BodyTokenKind as Kind;

        let tokens = syntax.tokens();
        for match_start in
            (0..tokens.len()).filter(|index| matches!(tokens[*index].kind(), Kind::Match))
        {
            let Some(open) = tokens[match_start + 1..]
                .iter()
                .position(|token| matches!(token.kind(), Kind::LeftBrace))
                .map(|offset| match_start + 1 + offset)
            else {
                continue;
            };
            let mut arm_start = open + 1;
            let mut index = arm_start;
            let mut depth = 0_usize;
            let mut pattern_end = None;

            while index < tokens.len() {
                match tokens[index].kind() {
                    Kind::LeftParen | Kind::LeftBracket | Kind::LeftBrace => depth += 1,
                    Kind::RightParen | Kind::RightBracket if depth > 0 => depth -= 1,
                    Kind::RightBrace if depth > 0 => depth -= 1,
                    Kind::RightBrace => break,
                    Kind::If if depth == 0 && pattern_end.is_none() => {
                        pattern_end = Some(index);
                    }
                    Kind::DoubleArrow if depth == 0 => {
                        collect_paths(&tokens[arm_start..pattern_end.unwrap_or(index)], output);
                        pattern_end = Some(index);
                    }
                    Kind::Comma if depth == 0 && pattern_end.is_some() => {
                        arm_start = index + 1;
                        pattern_end = None;
                    }
                    _ => {}
                }
                index += 1;
            }
        }
    }

    let mut output = Vec::new();
    match parsed {
        crate::item::Parsed::Function(function) => collect(&function.body_syntax, &mut output),
        crate::item::Parsed::Impl(crate::item::Impl { functions, .. })
        | crate::item::Parsed::Trait(crate::item::Trait { functions, .. }) => {
            for function in functions {
                collect(&function.body_syntax, &mut output);
            }
        }
        crate::item::Parsed::LegacyFunction(function) => {
            collect(&function.body_syntax, &mut output);
        }
        crate::item::Parsed::Module(_)
        | crate::item::Parsed::Use(_)
        | crate::item::Parsed::Struct(_)
        | crate::item::Parsed::Enum(_) => {}
    }
    output
}

/// Rust privacy: a private item declared in a module is visible from that
/// module and its descendants; `depth` is the length of the declaring path.
fn within(context: &Context, address: &Address, depth: usize) -> bool {
    address.package == context.namespace.package
        && context
            .namespace
            .modules
            .starts_with(&address.modules[..depth])
}

impl Context {
    fn address(&self, name: &str) -> Address {
        Address {
            package: self.namespace.package.clone(),
            modules: self.namespace.modules.clone(),
            name: name.into(),
        }
    }
}

impl Address {
    fn display(&self) -> ImmutableString {
        let mut segments = vec![self.package.as_str()];
        segments.extend(self.modules.iter().map(|segment| segment.as_str()));
        segments.push(self.name.as_str());
        segments.join("::").into()
    }
}

fn header(parsed: &crate::item::Parsed) -> Option<&crate::item::Header> {
    match parsed {
        crate::item::Parsed::Module(item) => Some(&item.header),
        crate::item::Parsed::Use(item) => Some(&item.header),
        crate::item::Parsed::Struct(item) => Some(&item.header),
        crate::item::Parsed::Enum(item) => Some(&item.header),
        crate::item::Parsed::Impl(item) => Some(&item.header),
        crate::item::Parsed::Trait(item) => Some(&item.header),
        crate::item::Parsed::Function(item) => Some(&item.header),
        crate::item::Parsed::LegacyFunction(_) => None,
    }
}

fn use_paths(item: &crate::item::Use) -> Vec<(crate::item::Name, Box<[ImmutableString]>)> {
    item.bindings()
}

fn path_segments(path: &crate::item::type_ref::Path) -> Box<[ImmutableString]> {
    path.segments
        .iter()
        .map(|segment| segment.name.text.clone())
        .collect()
}

fn host_target(
    host: &host::Registry,
    path: &[ImmutableString],
    reference: Span,
    #[cfg(feature = "rust-import")] native: Option<super::native::Site<'_>>,
) -> Result<Target, Error> {
    let qualified: ImmutableString = path.join("::").into();
    match host.entry(&qualified) {
        Some(host::Entry::Module) => Ok(Target::Module),
        Some(host::Entry::ModuleItem(definitions)) => Ok(Target::HostModuleItem {
            definitions: definitions.into(),
            owner: host.item_owner(&qualified).cloned(),
        }),
        Some(host::Entry::Type(definition)) => Ok(Target::Type(type_ref::Target::Host {
            definition: definition.clone(),
            owner: host.type_owner(&qualified).cloned(),
        })),
        Some(host::Entry::Item(definitions)) => Ok(Target::HostItem {
            definitions: definitions.into(),
            owner: host.item_owner(&qualified).cloned(),
        }),
        Some(host::Entry::Endpoint(contract)) => {
            let (_, owner) =
                host.operation_endpoint(&qualified)
                    .ok_or_else(|| Error::UnknownImport {
                        path: qualified.clone(),
                        reference,
                    })?;
            Ok(Target::HostEndpoint {
                contract: contract.clone(),
                owner: owner.clone(),
            })
        }
        // `use std::fmt;`, for hand-written `Debug` and `Display` impls.
        None if super::std_fmt_import(&qualified) => Ok(Target::Module),
        None => {
            #[cfg(feature = "rust-import")]
            let missing = native.filter(|site| site.missing_binding(host, &qualified));
            let error = Error::UnknownImport {
                path: qualified,
                reference,
            };
            #[cfg(feature = "rust-import")]
            if let Some(native) = missing {
                native.record(&error);
            }
            Err(error)
        }
    }
}

pub(super) fn collect_item_type_paths<'a>(
    item: &'a crate::item::Parsed,
    output: &mut Vec<&'a crate::item::type_ref::Path>,
) {
    match item {
        crate::item::Parsed::Struct(item) => {
            for field in &item.fields {
                collect_type_paths(&field.ty, output);
            }
        }
        crate::item::Parsed::Enum(item) => {
            for variant in &item.variants {
                for ty in &variant.payload {
                    collect_type_paths(ty, output);
                }
            }
        }
        crate::item::Parsed::Impl(item) => {
            collect_type_paths(&item.owner, output);
            // `impl component::Component<..> for C`: a facade trait by its
            // module.
            if let Some(trait_) = &item.trait_ {
                output.push(trait_);
            }
            for function in &item.functions {
                collect_function_type_paths(function, output);
            }
        }
        crate::item::Parsed::Trait(item) => {
            for function in &item.functions {
                collect_function_type_paths(function, output);
            }
        }
        crate::item::Parsed::Function(item) => collect_function_type_paths(item, output),
        crate::item::Parsed::Module(_)
        | crate::item::Parsed::Use(_)
        | crate::item::Parsed::LegacyFunction(_) => {}
    }
}

fn collect_function_type_paths<'a>(
    function: &'a crate::item::Function,
    output: &mut Vec<&'a crate::item::type_ref::Path>,
) {
    for parameter in &function.parameters {
        collect_type_paths(&parameter.ty, output);
    }
    if let Some(ty) = &function.output {
        collect_type_paths(ty, output);
    }
}

fn collect_type_paths<'a>(
    ty: &'a crate::item::type_ref::Type,
    output: &mut Vec<&'a crate::item::type_ref::Path>,
) {
    match ty {
        crate::item::type_ref::Type::Unit(_) => {}
        crate::item::type_ref::Type::Tuple { members, .. } => {
            for member in members {
                collect_type_paths(member, output);
            }
        }
        crate::item::type_ref::Type::Path(path) => {
            output.push(path);
            for segment in &path.segments {
                for argument in &segment.arguments {
                    if let crate::item::type_ref::Argument::Type(ty) = argument {
                        collect_type_paths(ty, output);
                    }
                }
            }
        }
        crate::item::type_ref::Type::Reference { target, .. } => {
            collect_type_paths(target, output);
        }
    }
}

fn finish_definitions(
    declarations: Vec<type_def::Declaration>,
    items: &[item::Shell],
) -> Result<Vec<type_def::Definition>, LocatedError> {
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
            | item::Kind::Function(_)
            | item::Kind::LegacyFunction(_) => {}
        }
    }
    Ok(declarations
        .into_iter()
        .map(|declaration| {
            let schema = schemas
                .remove(&declaration.id())
                .expect("each predeclared package type resolves exactly one schema");
            let capability_set = capabilities[&declaration.id()];
            declaration.resolve(schema, capability_set)
        })
        .collect())
}

impl Failure {
    fn from_located(error: LocatedError) -> Self {
        let (primary, secondary, error) = error.into_parts();
        Self {
            primary: source::Id::from_index(primary.index()),
            secondary: secondary.map(|source| source::Id::from_index(source.index())),
            error,
        }
    }
}
