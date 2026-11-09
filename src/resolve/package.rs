//! Aggregate semantic resolution for a validated static package graph.

use std::collections::{BTreeMap, BTreeSet};

use super::{item, type_ref, Error, LocatedError, Namespace, Private, Program, Script, Unresolved};
use crate::package::{graph, source, Graph};
use crate::{host, parse, program, type_def, ImmutableString, Span};

type UnitBindings = (
    Vec<Option<item::Use>>,
    BTreeMap<ImmutableString, type_ref::Target>,
    BTreeMap<ImmutableString, (source::Id, Span)>,
);

/// A name a `use` binds, and the path it names.
type UsePath = (crate::item::Name, Box<[ImmutableString]>);

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
}

impl Target {
    /// The namespace an import of the target binds its name in.
    const fn namespace(&self) -> item::Namespace {
        match self {
            Self::Module | Self::Trait | Self::Type(_) => item::Namespace::Type,
            Self::Item(_) | Self::HostItem { .. } => item::Namespace::Value,
            Self::HostModuleItem { .. } => item::Namespace::Both,
        }
    }
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
    /// The last segment, the item the path names in its owner.
    name_span: Span,
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
    /// A host's one in-memory file, which has no extern prelude to bound
    /// its paths ([`Graph::hosted`]).
    hosted: bool,
    declarations: BTreeMap<Address, Declaration>,
    exports: BTreeMap<Address, Export>,
    /// Private `use` bindings, so a path may start with an imported module.
    aliases: BTreeMap<Address, Export>,
    /// A second private `use` of a name `aliases` binds, in Rust's other
    /// namespace: the function `use badge::{self, badge};` binds beside
    /// the module (`through_alias`).
    other_aliases: BTreeMap<Address, Export>,
    /// The names each module's private glob imports bind (`use super::*;`,
    /// `use Shape::*;`), which its declarations and explicit imports
    /// shadow, as in Rust.
    globs: BTreeMap<Address, Export>,
    type_declarations: Vec<type_def::Declaration>,
    /// Each crate enum's variant names, for `use Shape::Circle;` and
    /// `use Shape::*;`.
    enum_variants: BTreeMap<type_def::Id, Box<[ImmutableString]>>,
    /// Each crate derive whose expansion the graph loaded (#593).
    expanded: crate::capability::Expanded,
    /// The marked types of a native root that rawr's copy of the crate
    /// mirrors, by their package and path (`Graph::island_mirrors`, #1249).
    #[cfg(feature = "rust-import")]
    mirrors: BTreeSet<Box<[ImmutableString]>>,
}

/// The variants of `std::cmp::Ordering`, which `use` may import by name.
pub(crate) const ORDERING_VARIANTS: [&str; 3] = ["Less", "Equal", "Greater"];

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
        let expanded = graph.expanded();
        let hosted = graph.hosted();
        #[cfg(feature = "rust-import")]
        let mirrors = graph.island_mirrors();
        #[cfg(feature = "rust-import")]
        let statics = graph.native_statics();
        let (packages, sources, parsed, entry_package, entry_source) = graph.into_parts();
        let package: ImmutableString = packages[entry_package.index()].name().into();
        #[cfg(feature = "rust-import")]
        let mirrors = mirrors
            .into_iter()
            .map(|path| std::iter::once(package.clone()).chain(path).collect())
            .collect();
        let evidence = sources
            .iter()
            .map(|source| source.diagnostic().clone())
            .collect();
        let mut package_by_name = BTreeMap::new();
        for package in &packages {
            package_by_name.insert(package.name().to_owned(), package.id());
        }

        #[cfg_attr(not(feature = "rust-import"), allow(unused_mut))]
        let mut units: Vec<Unit> = parsed
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
        // A marked static native code holds is native code's item, which
        // its module reads as if it were `pub use`d from there (#1291). One
        // whose type the bindings can't give stays a value of the
        // program's, as a `const` is.
        #[cfg(feature = "rust-import")]
        if let Some(app) = packages[entry_package.index()].rust_crate() {
            for path in &statics {
                let Some((name, modules)) = path.split_last() else {
                    continue;
                };
                let qualified = std::iter::once(app)
                    .chain(path.iter().map(ImmutableString::as_str))
                    .collect::<Vec<_>>()
                    .join("::");
                if !matches!(host.entry(&qualified), Some(host::Entry::Item(_))) {
                    continue;
                }
                let found = units.iter_mut().find(|unit| {
                    unit.context.package == entry_package
                        && *unit.context.namespace.modules == *modules
                });
                let Some(unit) = found else {
                    continue;
                };
                for item in unit.parsed.items_mut() {
                    if let crate::item::Parsed::Function(function) = item
                        && function.constant
                        && function.name.text == *name
                    {
                        *item = crate::item::Parsed::Use(native_static(function, app, modules));
                    }
                }
            }
        }

        let mut resolver = Self {
            #[cfg(feature = "rust-import")]
            native: None,
            host,
            package,
            packages,
            evidence,
            units,
            entry_source,
            hosted,
            declarations: BTreeMap::new(),
            exports: BTreeMap::new(),
            aliases: BTreeMap::new(),
            other_aliases: BTreeMap::new(),
            globs: BTreeMap::new(),
            type_declarations: Vec::new(),
            enum_variants: BTreeMap::new(),
            expanded,
            #[cfg(feature = "rust-import")]
            mirrors,
        };
        resolver.predeclare()?;
        resolver.predeclare_globs();
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

                if let crate::item::Parsed::TypeAlias(alias) = parsed {
                    self.insert_declaration(
                        context.address(&alias.name.text),
                        Declaration {
                            source: context.source,
                            span: alias.name.span,
                            public: alias.header.visibility.is_some(),
                            target: Target::Type(super::unresolved_alias(alias)),
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
                            if let crate::item::Parsed::Enum(declared) = parsed {
                                self.enum_variants.insert(
                                    definition,
                                    declared
                                        .variants
                                        .iter()
                                        .map(|variant| variant.name.text.clone())
                                        .collect(),
                                );
                            }
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
                        let address = context.address(&name.text);
                        let export = Export {
                            source: context.source,
                            context: context.clone(),
                            path,
                            span: name.span,
                        };
                        let aliases = if self.aliases.contains_key(&address) {
                            &mut self.other_aliases
                        } else {
                            &mut self.aliases
                        };
                        aliases.insert(address, export);
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

    /// Bind each private glob import's names, and the variants a public
    /// glob of an enum re-exports, until none binds more: a glob
    /// may reach names another glob bound, as `use Progress::*;` after
    /// `use super::*;` does, or a module's glob the names its parent's
    /// glob bound. A glob that names nothing binds nothing here; its
    /// module's bindings report it.
    fn predeclare_globs(&mut self) {
        loop {
            let mut found = BTreeMap::new();
            for unit in &self.units {
                let context = &unit.context;
                for parsed in unit.parsed.items() {
                    let crate::item::Parsed::Use(used) = parsed else {
                        continue;
                    };
                    let crate::item::UseTree::Glob { prefix, span } = &used.tree else {
                        continue;
                    };
                    let segments = path_segments(prefix);
                    // `pub use Shade::*;` re-exports the enum's variants.
                    let public = used.header.visibility.is_some();
                    if public && !self.enum_glob(context, &segments) {
                        continue;
                    }
                    for name in self.glob_names(context, &segments).unwrap_or_default() {
                        let address = context.address(&name);
                        if self.binds(&address) || found.contains_key(&address) {
                            continue;
                        }
                        let mut path = segments.to_vec();
                        path.push(name);
                        let export = Export {
                            source: context.source,
                            context: context.clone(),
                            path: path.into_boxed_slice(),
                            span: *span,
                        };
                        found.insert(address, (export, public));
                    }
                }
            }
            if found.is_empty() {
                return;
            }
            for (address, (export, public)) in found {
                let bound = if public {
                    &mut self.exports
                } else {
                    &mut self.globs
                };
                bound.insert(address, export);
            }
        }
    }

    /// Whether a declaration or an import binds `address`.
    fn binds(&self, address: &Address) -> bool {
        self.declarations.contains_key(address)
            || self.exports.contains_key(address)
            || self.aliases.contains_key(address)
            || self.globs.contains_key(address)
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
                    // `kinds::Secondary` after `pub use self::Kind::Secondary;`
                    // in `kinds`: the variant of the enum the import names.
                    let (origin_context, origin_path) = self.origin(&unit.context, &call.path);
                    if *origin_path != *call.path {
                        match self.variant(&origin_context, &origin_path) {
                            Some(item::Import::Variant { identity, .. }) => {
                                resolved.owners.insert((source, call.owner_span), identity);
                                continue;
                            }
                            // A crate's, spelled through its enum's path.
                            Some(item::Import::HostVariant { owner }) => {
                                let spelled = owner
                                    .split("::")
                                    .map(ImmutableString::from)
                                    .chain(call.path.last().cloned())
                                    .collect();
                                resolved.rust.insert((source, call.path_span), spelled);
                                continue;
                            }
                            _ => {}
                        }
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
                            | Target::Type(
                                type_ref::Target::Builtin
                                | type_ref::Target::Host { .. }
                                | type_ref::Target::Parameter
                                | type_ref::Target::Projection(..)
                                | type_ref::Target::Alias(_)
                                | type_ref::Target::Std(_),
                            ) => continue,
                        }
                    }

                    let mut active = BTreeSet::new();
                    match self
                        .resolve_path(&unit.context, &call.path, call.path_span, &mut active)
                        .map_err(|failure| self.not_in_module(&unit.context, &call, failure))?
                    {
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
                        | Target::Type(
                            type_ref::Target::Builtin
                            | type_ref::Target::Host { .. }
                            | type_ref::Target::Parameter
                            | type_ref::Target::Projection(..)
                            | type_ref::Target::Alias(_)
                            | type_ref::Target::Std(_),
                        ) => {}
                    }
                }
            }
        }
        Ok(resolved)
    }

    fn resolve(mut self) -> Result<Program, Failure> {
        super::std_trait::begin();
        self.validate_exports()?;
        let qualified_calls = self.resolve_qualified_calls()?;
        let qualified_pattern_owners = self.resolve_qualified_pattern_owners()?;
        let mut shells = Vec::new();
        let mut scripts = Vec::new();
        let entry = self.entry_source;
        let macros = super::macros::Table::new(
            self.units
                .iter()
                .filter(|unit| unit.context.source == entry)
                .chain(
                    self.units
                        .iter()
                        .filter(|unit| unit.context.source != entry),
                )
                .map(|unit| super::macros::Module {
                    source: program::source::Id::from_index(unit.context.source.index()),
                    package: &unit.context.namespace.package,
                    path: &unit.context.namespace.modules,
                    items: unit.parsed.items(),
                    macros: unit.parsed.macros(),
                    inner_attributes: unit.parsed.inner_attributes(),
                }),
        );

        self.resolve_aliases()?;
        let prelude = self.prelude();
        let mut aliases = BTreeMap::new();
        let mut lexical = BTreeMap::new();
        let units = std::mem::take(&mut self.units);
        for Unit {
            context,
            parsed,
            slots,
        } in units
        {
            lexical.insert(
                program::source::Id::from_index(context.source.index()),
                parsed.imports().to_vec().into_boxed_slice(),
            );
            let (source_map, parsed_items, statements, statement_source) = parsed.into_parts();
            if context.source == self.entry_source {
                super::async_main(&parsed_items, prelude.as_ref()).map_err(|error| Failure {
                    primary: context.source,
                    secondary: None,
                    error,
                })?;
            }

            if context.source != self.entry_source
                && let Some(position) = statements
            {
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
            // The host modules this unit's `use`s bind, by name: an imported
            // trait is one (`use freya::prelude::IntoElement;`).
            let modules = uses
                .iter()
                .flatten()
                .flat_map(|used| used.bindings.iter())
                .filter(|binding| {
                    matches!(
                        binding.target,
                        item::Import::Module | item::Import::ModuleItem { .. }
                    )
                })
                .map(|binding| (binding.name.text.clone(), binding.qualified.clone()))
                .collect::<BTreeMap<_, _>>();
            aliases.extend(super::source_aliases(
                program::source::Id::from_index(context.source.index()),
                &types,
            ));
            for (index, parsed) in parsed_items.into_vec().into_iter().enumerate() {
                let Some(slot) = &slots[index] else {
                    continue;
                };
                let parsed = {
                    let mut parsed = parsed;
                    qualify_opaque_bounds(&mut parsed, &modules);
                    write_renamed_modules(&mut parsed, &modules);
                    self.write_native_trait_objects(&context, &mut parsed, &modules);
                    parsed
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
                    error: self.unresolved_type(&context, error),
                })?;
                shells.push(shell);
            }
            scripts.push((
                context.source,
                source_map,
                Script {
                    source: program::source::Id::from_index(context.source.index()),
                    namespace: context.namespace.clone(),
                    text: statement_source,
                },
            ));
        }

        let definitions =
            finish_definitions(self.type_declarations, &shells, self.host, &self.expanded)
                .map_err(Failure::from_located)?;
        let (methods, builtin_methods, std_traits) =
            super::resolve_methods(&mut shells, &definitions, self.host)
                .map_err(Failure::from_located)?;
        super::resolve_functions(&mut shells).map_err(Failure::from_located)?;
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

        let entry = self
            .packages
            .iter()
            .find(|package| package.name() == self.package.as_str());
        let entry_crate = entry
            .and_then(|package| package.entry().and(package.rust_crate()))
            .map(ImmutableString::from);
        let features = entry
            .map(|package| package.features().clone())
            .unwrap_or_default();
        let program = Program {
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
            std_traits,
            qualified_functions: qualified_calls.functions,
            qualified_owners: qualified_calls.owners,
            qualified_pattern_owners,
            qualified_rust: qualified_calls.rust,
            entry_crate,
            #[cfg(feature = "rust-import")]
            mirrors: self.mirrors,
            crates: prelude,
            features,
            aliases,
            scripts,
            instances: super::Instances::default(),
            lexical,
            macros,
        };
        super::validate_invocation_storage(&program).map_err(Failure::from_located)?;
        super::infinite::Layouts::of(&program)
            .reject()
            .map_err(Failure::from_located)?;
        Ok(program)
    }

    /// Resolve each type alias's target in the module that declares it.
    /// An alias may name one another module declares, so the modules
    /// resolve theirs until every alias has its target or none more does.
    fn resolve_aliases(&mut self) -> Result<(), Failure> {
        let declares = |unit: &Unit| {
            unit.parsed
                .items()
                .iter()
                .any(|parsed| matches!(parsed, crate::item::Parsed::TypeAlias(_)))
        };
        if !self.units.iter().any(declares) {
            return Ok(());
        }
        loop {
            let mut resolved = false;
            let mut first = None;
            for index in 0..self.units.len() {
                if !declares(&self.units[index]) {
                    continue;
                }
                let context = self.units[index].context.clone();
                let items = self.units[index].parsed.items().to_vec();
                let (_, mut types, _) = self.resolve_unit_bindings(&context, &items)?;
                let before = items
                    .iter()
                    .filter(|parsed| {
                        matches!(parsed, crate::item::Parsed::TypeAlias(alias)
                            if matches!(types.get(&alias.name.text),
                                Some(type_ref::Target::Alias(type_ref::Alias { target: None, .. }))))
                    })
                    .count();
                let error = super::resolve_aliases(&items, &mut types, self.host);
                for parsed in &items {
                    let crate::item::Parsed::TypeAlias(alias) = parsed else {
                        continue;
                    };
                    let Some(type_ref::Target::Alias(done)) = types.get(&alias.name.text) else {
                        continue;
                    };
                    if done.target.is_none() {
                        continue;
                    }
                    if let Some(declaration) = self
                        .declarations
                        .get_mut(&context.address(&alias.name.text))
                        && matches!(
                            declaration.target,
                            Target::Type(type_ref::Target::Alias(type_ref::Alias {
                                target: None,
                                ..
                            }))
                        )
                    {
                        declaration.target = Target::Type(type_ref::Target::Alias(done.clone()));
                        resolved = true;
                    }
                }
                if let Some(error) = error
                    && first.is_none()
                    && before > 0
                {
                    first = Some(Failure {
                        primary: context.source,
                        secondary: None,
                        error,
                    });
                }
            }
            match first {
                None => return Ok(()),
                Some(failure) if !resolved => return Err(failure),
                Some(_) => {}
            }
        }
    }

    fn validate_exports(&self) -> Result<(), Failure> {
        for (address, export) in &self.exports {
            // `pub use self::Kind::Secondary;` re-exports a variant.
            let (context, path) = self.origin(&export.context, &export.path);
            if self.variant(&context, &path).is_some() {
                continue;
            }
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

    /// The names a glob import (`use dep::module::*;`) binds: every name
    /// of that module this unit sees that no declaration or explicit import
    /// of this unit already binds.
    fn glob_paths(
        &self,
        context: &Context,
        parsed: &crate::item::Use,
        bound: &BTreeMap<(ImmutableString, item::Namespace), (source::Id, Span, bool)>,
    ) -> Result<Vec<UsePath>, Failure> {
        let crate::item::UseTree::Glob { prefix, span } = &parsed.tree else {
            return Ok(Vec::new());
        };
        let segments = path_segments(prefix);
        let module: ImmutableString = segments.join("::").into();
        let local = self.enum_glob(context, &segments)
            || super::std_ordering(&module)
            || self.local_glob_module(context, &segments).is_some();
        if parsed.header.visibility.is_some() && !local {
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
        let Some(names) = self.glob_names(context, &segments) else {
            // rustc stops at a first segment that names no crate or module.
            let error = match prefix.segments.first() {
                Some(root) if self.unresolved_root(context, &root.name.text) => {
                    Error::UnknownImport {
                        path: root.name.text.clone(),
                        reference: root.name.span,
                        missing: Unresolved::Unlinked,
                    }
                }
                _ => Error::UnknownImport {
                    path: module,
                    reference: *span,
                    missing: Unresolved::Path,
                },
            };
            return Err(Failure {
                primary: context.source,
                secondary: None,
                error,
            });
        };
        Ok(names
            .into_iter()
            .filter(|name| {
                item::Namespace::Both
                    .sides()
                    .iter()
                    .any(|side| !bound.contains_key(&(name.clone(), *side)))
            })
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

    /// Every name a glob import of `segments` reaches from `context`: a
    /// crate enum's variants (`use Shape::*;`) or std `Ordering`'s; the
    /// names a module of this crate binds that `context` sees (`use
    /// super::*;`); or a std or crate module's names. `None` when the
    /// prefix names none of these.
    fn glob_names(
        &self,
        context: &Context,
        segments: &[ImmutableString],
    ) -> Option<Vec<ImmutableString>> {
        let module: ImmutableString = segments.join("::").into();
        if let Some((definition, _)) = self.script_enum(context, segments) {
            return Some(self.enum_variants[&definition].to_vec());
        }
        if super::std_ordering(&module) {
            return Some(
                ORDERING_VARIANTS
                    .iter()
                    .map(|name| ImmutableString::from(*name))
                    .collect(),
            );
        }
        if let Some(variants) = self
            .host
            .enum_variants(&super::std_reexport(&module, self.host))
        {
            return Some(variants);
        }
        if let Some(modules) = self.local_glob_module(context, segments) {
            return Some(self.module_names(context, &modules));
        }
        // `use std::collections::*;`: every name of that std module.
        if let Some(names) = super::std_path::glob(&module) {
            return Some(names);
        }
        matches!(
            self.host.entry(&module),
            Some(host::Entry::Module | host::Entry::ModuleItem(_))
        )
        .then(|| self.host.children(&module))
    }

    /// The names the crate module `modules` binds that `context` sees, as a
    /// glob import of it reaches them: its public items and re-exports,
    /// and, from inside it, its private items and imports too.
    fn module_names(&self, context: &Context, modules: &[ImmutableString]) -> Vec<ImmutableString> {
        let inside = context.namespace.modules.starts_with(modules);
        let at = |address: &Address| {
            address.package == context.namespace.package && *address.modules == *modules
        };
        let declared = self
            .declarations
            .iter()
            .filter(|(address, declaration)| at(address) && (declaration.public || inside))
            .map(|(address, _)| address);
        let exported = self.exports.keys().filter(|address| at(address));
        let imported = self
            .aliases
            .keys()
            .chain(self.globs.keys())
            .filter(|address| inside && at(address));
        declared
            .chain(exported)
            .chain(imported)
            .map(|address| address.name.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// Where a path of this crate's names its item: through each `use` or
    /// glob import that binds it, to the path that import names
    /// (`super::Circle` after the root's `use Shape::*;` is `Shape::Circle`
    /// in the root).
    fn origin(
        &self,
        context: &Context,
        path: &[ImmutableString],
    ) -> (Context, Box<[ImmutableString]>) {
        let mut context = context.clone();
        let mut path: Box<[ImmutableString]> = path.into();
        // Each step follows a distinct import, so a cycle ends the walk.
        for _ in 0..=self.aliases.len() + self.exports.len() + self.globs.len() {
            if let Some(import) = self.path_import(&context, &path) {
                context = import.context.clone();
                path = import.path.clone();
                continue;
            }
            // `keyboard::key` after `use iced::keyboard;`: the crate path
            // the first name's `use` starts it at (`crate_alias`).
            let Some((_, alias)) = self.crate_alias(&context, &path) else {
                break;
            };
            path = alias.path.iter().chain(&path[1..]).cloned().collect();
            context = alias.context.clone();
        }
        (context, path)
    }

    /// The `use` or glob import that binds the item `path` names in a
    /// module of this crate, when no declaration there does.
    fn path_import(&self, context: &Context, path: &[ImmutableString]) -> Option<&Export> {
        let (name, prefix) = path.split_last()?;
        let modules = self.local_glob_module(context, prefix)?;
        let address = Address {
            package: context.namespace.package.clone(),
            modules: modules.into_boxed_slice(),
            name: name.clone(),
        };
        if self.declarations.contains_key(&address) {
            return None;
        }
        self.exports
            .get(&address)
            .or_else(|| self.aliases.get(&address))
            .or_else(|| self.globs.get(&address))
    }

    /// Whether a glob import of `segments` names an enum's variants, the
    /// program's (`Shade::*`) or a crate's (`iced::Alignment::*`).
    fn enum_glob(&self, context: &Context, segments: &[ImmutableString]) -> bool {
        self.script_enum(context, segments).is_some()
            || self
                .host
                .enum_variants(&super::std_reexport(&segments.join("::"), self.host))
                .is_some()
    }

    /// The variant `path` names from `context`, after [`Self::origin`]:
    /// `Shape::Circle` of an enum of this crate, which `super::Circle`
    /// reaches through the parent's import, std's `Ordering::Less`, or a
    /// crate enum's, as `iced::Alignment::Center` or the re-exported
    /// `iced::Center`.
    fn variant(&self, context: &Context, path: &[ImmutableString]) -> Option<item::Import> {
        let (last, prefix) = path.split_last()?;
        let Some((definition, identity)) = self.script_enum(context, prefix) else {
            if super::std_ordering(&prefix.join("::")) && ORDERING_VARIANTS.contains(&last.as_str())
            {
                return Some(item::Import::OrderingVariant);
            }
            return self
                .host
                .enum_variant(&super::std_reexport(&path.join("::"), self.host))
                .map(|owner| item::Import::HostVariant { owner });
        };
        self.enum_variants[&definition]
            .contains(last)
            .then_some(item::Import::Variant {
                definition,
                identity,
            })
    }

    /// The crate enum a `use` prefix names, as `Shape`, `shapes::Shape` or
    /// `super::Shape`, or a name another `use` of this module imported.
    fn script_enum(
        &self,
        context: &Context,
        prefix: &[ImmutableString],
    ) -> Option<(type_def::Id, type_def::Identity)> {
        let target = match prefix {
            [] => return None,
            [name] => match self.declarations.get(&context.address(name)) {
                Some(declaration) => declaration.target.clone(),
                None => {
                    let address = context.address(name);
                    let alias = self
                        .through_alias(&address)
                        .or_else(|| self.globs.get(&address))?;
                    if alias.path.len() < 2 {
                        return None;
                    }
                    let mut active = BTreeSet::new();
                    self.resolve_any_path(&alias.context, &alias.path, alias.span, &mut active)
                        .ok()?
                }
            },
            _ => {
                if !self.is_script_path(context, prefix) {
                    return None;
                }
                let mut active = BTreeSet::new();
                self.resolve_any_path(context, prefix, Span::NONE, &mut active)
                    .ok()?
            }
        };
        match target {
            Target::Type(type_ref::Target::Script {
                definition,
                identity,
            }) if self.enum_variants.contains_key(&definition) => Some((definition, identity)),
            _ => None,
        }
    }

    /// The modules of this crate a glob import's prefix names: `super`,
    /// `super::super`, `crate`, `crate::a`, `self::a` or a child module.
    fn local_glob_module(
        &self,
        context: &Context,
        segments: &[ImmutableString],
    ) -> Option<Vec<ImmutableString>> {
        let segments = through_self(segments);
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
        // Each name bound in each of Rust's namespaces, which an import of
        // that name in that namespace collides with, and whether an import
        // bound it: two imports are rustc's E0252.
        let mut bound =
            BTreeMap::<(ImmutableString, item::Namespace), (source::Id, Span, bool)>::new();
        for (address, declaration) in &self.declarations {
            if address.package == context.namespace.package
                && address.modules == context.namespace.modules
            {
                let namespace = match declaration.target {
                    Target::Module | Target::Trait => item::Namespace::Type,
                    Target::Item(_) => item::Namespace::Value,
                    _ => item::Namespace::Both,
                };
                for side in namespace.sides() {
                    bound.insert(
                        (address.name.clone(), *side),
                        (declaration.source, declaration.span, false),
                    );
                }
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

            let globbed = self.glob_paths(context, parsed, &bound)?;
            for (name, path) in use_paths(parsed).into_iter().chain(globbed) {
                let module_only = parsed.binds_module_only(&name);
                // `use ::image::ColorType;` names the crate `image`, never a
                // module of this crate or one another `use` brought in.
                if parsed.global() {
                    self.reject_local_global(context, &path, super::prefix_span(&parsed.tree))?;
                }
                // `use keyboard::key;` after `use iced::keyboard;`: a path
                // through a library module this unit imported, as Rust's
                // paths start at a name in scope.
                let path = match path.split_first() {
                    Some((first, rest)) if !rest.is_empty() && !parsed.global() => uses
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
                let (origin_context, origin_path) = self.origin(context, &path);
                let variant = self.variant(&origin_context, &origin_path);
                // A std or crate path another module's import names, as
                // `use super::*;` reaches the root's `use
                // std::collections::HashMap;`, is spelled as that import
                // spells it.
                let spelled = if self.is_script_path(&origin_context, &origin_path) {
                    &path
                } else {
                    &origin_path
                };
                // A crate's derive macro and nothing else at its path, as
                // `use serde::Deserialize;`.
                let variant = variant.or_else(|| {
                    let qualified = super::std_reexport(&spelled.join("::"), self.host);
                    (self.host.entry(&qualified).is_none() && self.host.derive_macro(&qualified))
                        .then_some(item::Import::Derive)
                });
                let target = match variant {
                    Some(_) => Target::Module,
                    None => {
                        let mut active = BTreeSet::new();
                        self.resolve_any_path(context, &path, name.span, &mut active)
                            .map_err(|failure| {
                                self.unresolved_use(
                                    context,
                                    &parsed.tree,
                                    &path,
                                    name.span,
                                    failure,
                                )
                            })?
                    }
                };
                // A glob's name is shadowed by the one a declaration or an
                // explicit import binds in its namespace, as in Rust: `use
                // m::badge;` beside `use m::badge::*;` keeps the glob's
                // function `badge`.
                let namespace = variant
                    .as_ref()
                    .map_or_else(|| target.namespace(), item::Import::namespace);
                if glob(&parsed_items[index])
                    && namespace
                        .sides()
                        .iter()
                        .any(|side| bound.contains_key(&(name.text.clone(), *side)))
                {
                    continue;
                }
                // An item of a `.rs` module keeps its crate spelling. A Rust
                // type another module of this crate imported, named through
                // that import (`use crate::Preset;` after the root's `use
                // preset::Preset;`), keeps the type's own spelling, so its
                // members are the ones the root's name reaches.
                let reexported = match &target {
                    Target::Type(type_ref::Target::Host { definition, .. })
                        if definition.identity().imported().is_some()
                            && matches!(
                                path.first().map(ImmutableString::as_str),
                                Some("crate" | "super" | "self")
                            ) =>
                    {
                        Some(definition.path())
                    }
                    _ => None,
                };
                let qualified = super::std_reexport(
                    &self.rust_spelling(context, &path).map_or_else(
                        || reexported.unwrap_or_else(|| spelled.join("::")),
                        |spelling| spelling.join("::"),
                    ),
                    self.host,
                );
                let import = match target {
                    _ if let Some(variant) = variant.clone() => variant,
                    Target::Module | Target::Trait => item::Import::Module,
                    // `use a::m::{self}` imports the module `m` only.
                    Target::HostModuleItem { .. } if module_only => item::Import::Module,
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
                    // A std type Rawr doesn't represent, through a module of
                    // this crate that re-exports it.
                    Target::Type(type_ref::Target::Std(path)) => {
                        types.insert(name.text.clone(), type_ref::Target::Std(path));
                        item::Import::Module
                    }
                    Target::Type(type_ref::Target::Alias(alias)) => {
                        types.insert(name.text.clone(), type_ref::Target::Alias(alias));
                        item::Import::Alias
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
                };
                occupied.insert(name.text.clone(), (context.source, name.span));
                let previous = import.namespace().sides().iter().find_map(|side| {
                    bound.insert(
                        (name.text.clone(), *side),
                        (context.source, name.span, true),
                    )
                });
                if let Some((previous_source, previous_span, imported)) = previous {
                    return Err(Failure {
                        primary: context.source,
                        secondary: Some(previous_source),
                        error: Error::duplicate(name.text, name.span, previous_span, imported),
                    });
                }
                // A std type imported under another name, as iced's `Never`
                // is std's `Infallible`, names Rawr's type for that std path.
                if matches!(import, item::Import::Module)
                    && let Some(builtin) = super::renamed_std_type(&qualified, &name.text)
                {
                    types.insert(
                        name.text.clone(),
                        type_ref::Target::Alias(type_ref::Alias::builtin(builtin, name.span)),
                    );
                }
                // A std type names its std path, which a path through it
                // resolves by, unless it names one of Rawr's types above.
                if matches!(import, item::Import::Module)
                    && !types.contains_key(&name.text)
                    && let Some(path) = super::std_path::bound_type(&qualified)
                {
                    types.insert(name.text.clone(), type_ref::Target::Std(path));
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
            if path.global {
                self.reject_local_global(context, &segments, path.span)?;
                continue;
            }
            if segments.len() < 2 || !self.is_script_path(context, &segments) {
                continue;
            }
            let mut active = BTreeSet::new();
            let target = self
                .resolve_path(context, &segments, path.span, &mut active)
                .map_err(|failure| self.unresolved_type_path(context, path, failure))?;
            match target {
                Target::Type(target) => {
                    types.insert(segments.join("::").into(), target);
                }
                Target::Module
                | Target::Trait
                | Target::HostModuleItem { .. }
                | Target::Item(_)
                | Target::HostItem { .. } => {}
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
            first == "crate"
                || first == "super"
                || first == "self" && path.len() > 1
                || self.is_child_module(context, first)
        })
    }

    /// A `use` binding of a script module in `context` (`use super::config;`),
    /// which a longer path may start with.
    fn module_alias(&self, context: &Context, name: &str) -> Option<&Export> {
        let address = context.address(name);
        let alias = self
            .through_alias(&address)
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

    /// The `use` in `context` that binds the first name of the longer path
    /// `path` (`use iced::keyboard;` for `keyboard::key`), with its address.
    /// A crate path `path` names itself is its own, as `::iced::..` is: a
    /// name that is both is rustc's E0659, which no checked program has.
    fn crate_alias(
        &self,
        context: &Context,
        path: &[ImmutableString],
    ) -> Option<(Address, &Export)> {
        let [first, _, ..] = path else {
            return None;
        };
        let address = context.address(first);
        let alias = self
            .through_alias(&address)
            .or_else(|| self.exports.get(&address))?;
        let direct = host_target(
            self.host,
            path,
            Span::NONE,
            #[cfg(feature = "rust-import")]
            None,
        );
        (alias.path.first() != Some(first) && direct.is_err()).then_some((address, alias))
    }

    /// The private `use` of `address` a longer path goes through: of two
    /// `use`s of one name in Rust's two namespaces, the one that names a
    /// module, as `badge::Secondary` goes through the module of `use
    /// badge::{self, badge};` and `badge(..)` calls the function.
    fn through_alias(&self, address: &Address) -> Option<&Export> {
        let first = self.aliases.get(address)?;
        let Some(other) = self.other_aliases.get(address) else {
            return Some(first);
        };
        let module = |alias: &Export| {
            alias.path.first() != Some(&address.name)
                && matches!(
                    self.resolve_any_path(
                        &alias.context,
                        &alias.path,
                        alias.span,
                        &mut BTreeSet::new()
                    ),
                    Ok(Target::Module | Target::HostModuleItem { .. })
                )
        };
        Some(if !module(first) && module(other) {
            other
        } else {
            first
        })
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
        } else if let Some((address, alias)) = self.crate_alias(context, path)
            && active.insert(address.clone())
        {
            // `use keyboard::key;` after `use iced::keyboard;`, wherever it
            // is resolved from, as a child module's `use super::*;` reaches
            // it: the path starts at the name in scope, as rustc's do.
            let expanded = alias
                .path
                .iter()
                .chain(&path[1..])
                .cloned()
                .collect::<Vec<_>>();
            let target = self.resolve_any_path(&alias.context, &expanded, reference, active);
            active.remove(&address);
            target
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
        let mut item = modules.to_vec();
        item.push(name.clone());
        if !package.is_native_path(&item) {
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

    /// Each trait object of `item`'s signatures whose trait is native, as
    /// `dyn crate::page::Page`, `dyn page::Page` after `use crate::page;`
    /// or `dyn Page` after `use crate::page::Page;`, written from the app
    /// crate's root (`dyn polyglot::page::Page`), so the resolver finds the
    /// trait by its path (`type_ref::native_trait`). The trait's own
    /// arguments stay on its segment.
    fn write_native_trait_objects(
        &self,
        context: &Context,
        item: &mut crate::item::Parsed,
        modules: &BTreeMap<ImmutableString, ImmutableString>,
    ) {
        use crate::item::type_ref::{Path, Segment};
        let path = |path: &mut Path| {
            let [dyn_, first, ..] = &path.segments[..] else {
                return;
            };
            if dyn_.name.text.as_str() != crate::item::type_ref::DYN_SEGMENT {
                return;
            }
            let written = path.segments[1..]
                .iter()
                .map(|segment| segment.name.text.clone())
                .collect::<Vec<_>>();
            let spelled = match modules.get(&first.name.text) {
                Some(module) => module
                    .split("::")
                    .map(ImmutableString::from)
                    .chain(written[1..].iter().cloned())
                    .collect(),
                None => match self.rust_spelling(context, &written) {
                    Some(spelled) => spelled,
                    None => return,
                },
            };
            if spelled.len() < 2 {
                return;
            }
            let last = path.segments[path.segments.len() - 1].clone();
            let (at, span) = (first.name.span, first.span);
            let count = spelled.len();
            path.segments = std::iter::once(dyn_.clone())
                .chain(spelled.into_iter().enumerate().map(|(index, text)| {
                    if index + 1 == count {
                        Segment {
                            name: crate::item::Name {
                                text,
                                span: last.name.span,
                            },
                            ..last.clone()
                        }
                    } else {
                        Segment {
                            name: crate::item::Name { text, span: at },
                            arguments: Box::default(),
                            span,
                        }
                    }
                }))
                .collect();
        };
        each_signature_path(item, &path);
    }

    /// `crate::…`, `super::…` or child-module paths that land in a `.rs`
    /// module of the current package, or name one, spelled through the
    /// app's crate name. A `use` that binds a `.rs` module (`use
    /// crate::units;`) so binds the app crate's module, and paths through
    /// the binding (`units::BONUS`) reach its items as rustc's do.
    fn rust_spelling(
        &self,
        context: &Context,
        path: &[ImmutableString],
    ) -> Option<Vec<ImmutableString>> {
        let package = &self.packages[context.package.index()];
        let alias = package.rust_crate()?;
        let path = through_self(path);
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
        } else if first == "self" && path.len() > 1 {
            absolute.extend_from_slice(modules);
            absolute.extend_from_slice(&path[1..]);
        } else if self.is_child_module(context, first) {
            absolute.extend_from_slice(modules);
            absolute.extend_from_slice(path);
        } else {
            return None;
        }
        package.names_native(&absolute).then(|| {
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
        let path = through_self(path);
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
        } else if first == "self" && path.len() > 1 {
            (context.package, modules, &path[1..])
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
        // A private `use` is visible to the module's descendants, as in
        // Rust: `use crate::Preset;` inside `mod grid` reaches the root's
        // `use preset::Preset;`.
        if let Some(alias) = self.aliases.get(&address)
            && within(context, &address, address.modules.len())
        {
            self.validate_module_hops(context, &address, reference)?;
            if !active.insert(address.clone()) {
                return Err(Failure {
                    primary: context.source,
                    secondary: Some(alias.source),
                    error: Error::InvalidImport {
                        path: path.join("::").into(),
                        reason: "cyclic import".into(),
                        reference,
                        declaration: None,
                    },
                });
            }
            let target = self.resolve_any_path(&alias.context, &alias.path, alias.span, active)?;
            active.remove(&address);
            return Ok(target);
        }
        // A name a private glob import binds, which the module's
        // descendants see as they see its private `use`s.
        if let Some(glob) = self.globs.get(&address)
            && within(context, &address, address.modules.len())
            && active.insert(address.clone())
        {
            self.validate_module_hops(context, &address, reference)?;
            let target = self.resolve_any_path(&glob.context, &glob.path, glob.span, active)?;
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
            return Err(self.private(context, address, declaration, reference));
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
            let module_address = Address {
                package: address.package.clone(),
                modules: address.modules[..depth].into(),
                name: address.modules[depth].clone(),
            };
            let Some(module) = self.declarations.get(&module_address) else {
                return Err(self.unknown_import(context, &[], reference));
            };
            if !module.public && !within(context, address, depth) {
                return Err(self.private(context, &module_address, module, reference));
            }
        }
        Ok(())
    }

    /// rustc's E0603 for a path to `declaration`, an item private to a
    /// module `context` is outside of.
    fn private(
        &self,
        context: &Context,
        address: &Address,
        declaration: &Declaration,
        reference: Span,
    ) -> Failure {
        let item = match &declaration.target {
            Target::Module => Private::Module,
            Target::Trait => Private::Trait,
            Target::Type(type_ref::Target::Script { definition, .. })
                if self.enum_variants.contains_key(definition) =>
            {
                Private::Enum
            }
            Target::Type(type_ref::Target::Alias(_)) => Private::TypeAlias,
            Target::Type(_) => Private::Struct,
            Target::Item(_) | Target::HostItem { .. } | Target::HostModuleItem { .. } => {
                Private::Function
            }
        };
        Failure {
            primary: context.source,
            secondary: Some(declaration.source),
            error: Error::PrivateItem {
                item,
                name: address.name.clone(),
                reference: last_segment(reference, &address.name),
                declaration: Some(declaration.span),
            },
        }
    }

    /// A path that starts with `::` names an outside crate, as in Rust:
    /// `::config::GAP` is an error when `config` is a module of this crate.
    fn reject_local_global(
        &self,
        context: &Context,
        path: &[ImmutableString],
        reference: Span,
    ) -> Result<(), Failure> {
        let Some(first) = path.first() else {
            return Ok(());
        };
        let local = matches!(first.as_str(), "crate" | "self" | "super" | "Self")
            || self.is_script_path(context, path)
            || self.module_alias(context, first).is_some();
        if !local {
            return Ok(());
        }
        Err(Failure {
            primary: context.source,
            secondary: None,
            error: super::not_a_crate(first, reference),
        })
    }

    /// A path in a body whose module exists but has no item of its last
    /// segment's name is rustc's E0425 "cannot find value `X` in module
    /// `m`", not the E0432 of an unresolved `use`.
    fn not_in_module(&self, context: &Context, call: &QualifiedCall, failure: Failure) -> Failure {
        let Error::UnknownImport { .. } = failure.error else {
            return failure;
        };
        let Some((item, owner)) = call.path.split_last() else {
            return failure;
        };
        let mut active = BTreeSet::new();
        match self.resolve_path(context, owner, call.owner_span, &mut active) {
            Ok(Target::Module) => Failure {
                error: Error::NotInModule {
                    module: owner.join("::").into(),
                    item: item.clone(),
                    reference: call.name_span,
                },
                ..failure
            },
            _ => failure,
        }
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
                missing: Unresolved::Path,
            },
        }
    }

    /// The crates this package's paths may start with, as rustc's extern
    /// prelude has them. `None` for a host's one in-memory file, whose host
    /// decides what it reaches.
    fn prelude(&self) -> Option<super::extern_prelude::Prelude> {
        self.packages
            .iter()
            .filter(|_| !self.hosted)
            .find(|package| package.name() == self.package.as_str())
            .map(super::extern_prelude::Prelude::of)
    }

    /// Whether `root`, a path's first segment in `context`, names no crate,
    /// module or item in scope, where rustc stops resolving the path.
    fn unresolved_root(&self, context: &Context, root: &str) -> bool {
        let address = context.address(root);
        // A `use` binds the name, unless it is `use root;` itself.
        let bound = |uses: &BTreeMap<Address, Export>| {
            uses.get(&address).is_some_and(|export| {
                export.path.first().map(ImmutableString::as_str) != Some(root)
            })
        };
        !matches!(
            root,
            "std" | "core" | "alloc" | "crate" | "self" | "super" | "Self"
        ) && !self.declarations.contains_key(&address)
            && !bound(&self.exports)
            && !bound(&self.aliases)
            && !bound(&self.globs)
            && self.host.entry(root).is_none()
            && self
                .prelude()
                .is_none_or(|crates| crates.reach(root, Span::NONE).is_err())
    }

    /// Whether `path` in `context` names a module of this crate or of
    /// another. A probe: a crate's miss it finds isn't recorded.
    fn names_module(&self, context: &Context, path: &[ImmutableString]) -> bool {
        let target = if self.is_script_path(context, path) {
            self.resolve_path(context, path, Span::NONE, &mut BTreeSet::new())
                .ok()
        } else {
            host_target(
                self.host,
                path,
                Span::NONE,
                #[cfg(feature = "rust-import")]
                None,
            )
            .ok()
        };
        matches!(target, Some(Target::Module | Target::HostModuleItem { .. }))
    }

    /// The longest proper prefix of `path` in `context` that names a module,
    /// by its length.
    fn found_module(&self, context: &Context, path: &[ImmutableString]) -> Option<usize> {
        (1..path.len())
            .rev()
            .find(|&end| self.names_module(context, &path[..end]))
    }

    /// The module `module` names in `context`, as rustc's E0432 names it:
    /// its path from its crate's root, or the crate's name for a crate's
    /// root.
    fn rustc_module(&self, context: &Context, module: &[ImmutableString]) -> ImmutableString {
        let here = context.namespace.modules.as_ref();
        let supers = module
            .iter()
            .take_while(|segment| *segment == "super")
            .count();
        let absolute = match module {
            [first, rest @ ..] if first == "crate" => rest.to_vec(),
            [first, rest @ ..] if first == "self" => [here, rest].concat(),
            _ if supers > 0 => {
                let parent = &here[..here.len().saturating_sub(supers)];
                [parent, &module[supers..]].concat()
            }
            [first, ..] if self.is_child_module(context, first) => [here, module].concat(),
            // Another crate's module, named from its root.
            [_, rest @ ..] if !rest.is_empty() && !self.is_script_path(context, module) => {
                rest.to_vec()
            }
            _ => module.to_vec(),
        };
        absolute.join("::").into()
    }

    /// rustc's E0432, or its E0433 where the path's first segment names no
    /// crate, for the `use` of `path` (written as `tree`, its name at
    /// `reference`) that `failure` refuses with no more than its path.
    fn unresolved_use(
        &self,
        context: &Context,
        tree: &crate::item::UseTree,
        path: &[ImmutableString],
        reference: Span,
        failure: Failure,
    ) -> Failure {
        let Error::UnknownImport {
            reference: at,
            missing: Unresolved::Path,
            ..
        } = &failure.error
        else {
            return failure;
        };
        if *at != reference {
            return failure;
        }
        let written = match tree {
            crate::item::UseTree::Path(prefix, _)
            | crate::item::UseTree::Group { prefix, .. }
            | crate::item::UseTree::Glob { prefix, .. } => &prefix.segments,
        };
        // The span of the segment at `index`, as the `use` writes it.
        let segment = |index: usize| {
            written
                .get(index)
                .filter(|segment| path.get(index) == Some(&segment.name.text))
                .map(|segment| segment.name.span)
        };
        let error = match path {
            [root] if self.unresolved_root(context, root) => Error::UnknownImport {
                path: root.clone(),
                reference,
                missing: Unresolved::Crate,
            },
            [root, _, ..] if self.unresolved_root(context, root) => match segment(0) {
                Some(span) => Error::UnknownType {
                    name: root.clone(),
                    reference: span,
                    path: true,
                },
                None => return failure,
            },
            _ => match self.found_module(context, path) {
                Some(end) if end + 1 == path.len() => Error::UnknownImport {
                    path: path.join("::").into(),
                    reference,
                    missing: Unresolved::Item(self.rustc_module(context, &path[..end])),
                },
                Some(end) if let Some(span) = segment(end) => Error::UnknownImport {
                    path: path[..=end].join("::").into(),
                    reference: span,
                    missing: Unresolved::Module(self.rustc_module(context, &path[..end])),
                },
                _ => return failure,
            },
        };
        Failure { error, ..failure }
    }

    /// rustc's E0425 for the type path `path` into a module of this crate
    /// that `failure` refuses as an unresolved import: the module has no
    /// type of its last segment's name.
    fn unresolved_type_path(
        &self,
        context: &Context,
        path: &crate::item::type_ref::Path,
        failure: Failure,
    ) -> Failure {
        let segments = path_segments(path);
        match (&failure.error, path.segments.last()) {
            (
                Error::UnknownImport {
                    reference,
                    missing: Unresolved::Path,
                    ..
                },
                Some(last),
            ) if *reference == path.span
                && self.found_module(context, &segments) == Some(segments.len() - 1) =>
            {
                Failure {
                    error: Error::UnknownType {
                        name: segments.join("::").into(),
                        reference: last.name.span,
                        path: false,
                    },
                    ..failure
                }
            }
            _ => failure,
        }
    }

    /// rustc's error for a type path another crate's module doesn't
    /// resolve, which `error` reports at the whole path: E0433 at its first
    /// segment where that names no crate or module in scope, else E0425 at
    /// its last.
    fn unresolved_type(&self, context: &Context, error: Error) -> Error {
        let Error::UnknownType {
            name,
            reference,
            path: false,
        } = &error
        else {
            return error;
        };
        let (Some((root, _)), Some((_, last))) = (name.split_once("::"), name.rsplit_once("::"))
        else {
            return error;
        };
        let start = reference.start();
        // The spans below are the path's own only where it is written as
        // `name` is, on one line.
        if text_end(start, name) != reference.end() {
            return error;
        }
        if self.unresolved_root(context, root) {
            return Error::UnknownType {
                name: root.into(),
                reference: Span::new(start, text_end(start, root)),
                path: true,
            };
        }
        let last_start = text_end(start, &name[..name.len() - last.len()]);
        Error::UnknownType {
            name: name.clone(),
            reference: Span::new(last_start, reference.end()),
            path: false,
        }
    }
}

/// The position `text` ends at when it starts at `position` on one line.
fn text_end(mut position: crate::Position, text: &str) -> crate::Position {
    for _ in text.chars() {
        position.advance();
    }
    position
}

/// `self::super::m` is `super::m`: a leading `self` stays in the module
/// the next `super` leaves.
fn through_self(path: &[ImmutableString]) -> &[ImmutableString] {
    match path {
        [first, second, ..] if first == "self" && second == "super" => &path[1..],
        _ => path,
    }
}

/// The first segment of a path in a body: a name, or `self` before `::`,
/// which names the current module as rustc reads it.
fn path_head(tokens: &[crate::item::BodyToken], index: usize) -> Option<ImmutableString> {
    use crate::item::BodyTokenKind as Kind;

    match tokens[index].kind() {
        Kind::Identifier(name) => Some(name.clone()),
        Kind::SelfValue
            if tokens
                .get(index + 1)
                .is_some_and(|token| matches!(token.kind(), Kind::DoubleColon)) =>
        {
            Some("self".into())
        }
        _ => None,
    }
}

fn qualified_calls(parsed: &crate::item::Parsed) -> Vec<QualifiedCall> {
    fn collect(syntax: &crate::item::BodySyntax, output: &mut Vec<QualifiedCall>) {
        let tokens = syntax.tokens();
        let mut index = 0;

        while index < tokens.len() {
            let Some(first) = path_head(tokens, index) else {
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

            let mut segments = vec![(first, tokens[index].span())];
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
                    name_span: segments[segments.len() - 1].1,
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
        crate::item::Parsed::Module(_)
        | crate::item::Parsed::Use(_)
        | crate::item::Parsed::Struct(_)
        | crate::item::Parsed::Enum(_)
        | crate::item::Parsed::TypeAlias(_) => {}
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
            let Some(first) = path_head(tokens, index) else {
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

            let mut segments = vec![(first, tokens[index].span())];
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
        crate::item::Parsed::Module(_)
        | crate::item::Parsed::Use(_)
        | crate::item::Parsed::Struct(_)
        | crate::item::Parsed::Enum(_)
        | crate::item::Parsed::TypeAlias(_) => {}
    }
    output
}

/// The end of `reference` that names `name`, where rustc's E0603 points:
/// `make_sausage` of `sausage_factory::make_sausage`.
fn last_segment(reference: Span, name: &str) -> Span {
    let (start, end) = (reference.start(), reference.end());
    let length = u16::try_from(name.chars().count()).unwrap_or(u16::MAX);
    match (start.line(), end.line(), end.position()) {
        (Some(first), Some(last), Some(column))
            if first == last && column > usize::from(length) =>
        {
            let line = u16::try_from(last).unwrap_or(u16::MAX);
            let column = u16::try_from(column).unwrap_or(u16::MAX);
            Span::new(crate::Position::new(line, column - length), end)
        }
        _ => reference,
    }
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

fn header(parsed: &crate::item::Parsed) -> Option<&crate::item::Header> {
    match parsed {
        crate::item::Parsed::Module(item) => Some(&item.header),
        crate::item::Parsed::Use(item) => Some(&item.header),
        crate::item::Parsed::Struct(item) => Some(&item.header),
        crate::item::Parsed::Enum(item) => Some(&item.header),
        crate::item::Parsed::Impl(item) => Some(&item.header),
        crate::item::Parsed::Trait(item) => Some(&item.header),
        crate::item::Parsed::Function(item) => Some(&item.header),
        crate::item::Parsed::TypeAlias(item) => Some(&item.header),
    }
}

fn use_paths(item: &crate::item::Use) -> Vec<UsePath> {
    item.bindings()
}

fn path_segments(path: &crate::item::type_ref::Path) -> Box<[ImmutableString]> {
    path.segments
        .iter()
        .map(|segment| segment.name.text.clone())
        .collect()
}

/// The `pub use app::modules::NAME;` a marked static native code holds
/// stands for (#1291), written where the static is, so a read of it, by
/// any path, is a read of native code's item in place.
#[cfg(feature = "rust-import")]
fn native_static(
    function: &crate::item::Function,
    app: &str,
    modules: &[ImmutableString],
) -> crate::item::Use {
    use crate::item::type_ref::{Path, Segment};
    let name = &function.name;
    let segment = |text: ImmutableString| Segment {
        name: crate::item::Name {
            text,
            span: name.span,
        },
        arguments: Box::default(),
        span: name.span,
    };
    let segments = std::iter::once(ImmutableString::from(app))
        .chain(modules.iter().cloned())
        .chain(std::iter::once(name.text.clone()))
        .map(segment)
        .collect();
    crate::item::Use {
        scope: None,
        header: function.header.clone(),
        tree: crate::item::UseTree::Path(
            Path {
                segments,
                span: name.span,
                global: false,
            },
            None,
        ),
        span: function.span,
    }
}

fn host_target(
    host: &host::Registry,
    path: &[ImmutableString],
    reference: Span,
    #[cfg(feature = "rust-import")] native: Option<super::native::Site<'_>>,
) -> Result<Target, Error> {
    let qualified = super::std_reexport(&path.join("::"), host);
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
        Some(host::Entry::Builtin(builtin)) => Ok(Target::Type(type_ref::Target::Alias(
            type_ref::Alias::builtin(builtin, reference),
        ))),
        // A crate's type alias, as `use iced::Result;`.
        Some(host::Entry::TypeAlias(alias)) => type_ref::Alias::host(alias, host, reference)
            .map(|alias| Target::Type(type_ref::Target::Alias(alias)))
            .ok_or_else(|| Error::UnknownType {
                name: qualified.clone(),
                reference,
                path: false,
            }),
        Some(host::Entry::Item(definitions)) => Ok(Target::HostItem {
            definitions: definitions.into(),
            owner: host.item_owner(&qualified).cloned(),
        }),
        // `use tokio::join;`, a macro Rawr expands.
        None if super::tokio_macro(&qualified, host).is_some()
            || qualified.contains("::") && super::hasher_alias(&qualified).is_some() =>
        {
            Ok(Target::Module)
        }
        // A path into std, which its uses name: `use std::fmt;` for
        // hand-written `Debug` and `Display` impls.
        None if super::std_path::import(&qualified, reference)?.is_some() => Ok(Target::Module),
        None => {
            #[cfg(feature = "rust-import")]
            let missing = native.filter(|site| site.missing_binding(host, &qualified));
            let error = Error::UnknownImport {
                path: qualified,
                reference,
                missing: Unresolved::Path,
            };
            #[cfg(feature = "rust-import")]
            if let Some(native) = missing {
                native.record(&error);
            }
            Err(error)
        }
    }
}

/// A return type's `impl Trait` bound that starts at a host module a `use`
/// binds (`impl IntoElement` after `use freya::prelude::IntoElement;`) is
/// written out from the crate's root, `impl freya::prelude::IntoElement`,
/// so the resolver finds the imported trait by its path
/// (`type_ref::OPAQUE_PREFIX`).
fn qualify_opaque_bounds(
    item: &mut crate::item::Parsed,
    modules: &BTreeMap<ImmutableString, ImmutableString>,
) {
    use crate::item::type_ref::{Argument, Segment, Type, IMPL_TYPE};
    fn qualify(ty: &mut Type, modules: &BTreeMap<ImmutableString, ImmutableString>) {
        match ty {
            Type::Path(path) => {
                let opaque = matches!(&path.segments[..], [segment]
                    if segment.name.text == IMPL_TYPE);
                for segment in path.segments.iter_mut() {
                    for argument in segment.arguments.iter_mut() {
                        let Argument::Type(argument) = argument else {
                            continue;
                        };
                        if opaque
                            && let Type::Path(bound) = argument
                            && !bound.global
                            && let Some(module) = bound
                                .segments
                                .first()
                                .and_then(|first| modules.get(&first.name.text))
                        {
                            let (at, span) = (bound.segments[0].name.span, bound.segments[0].span);
                            // The first segment's arguments (`Widget<Message>`)
                            // stay on the segment that names the trait.
                            let written = bound.segments[0].arguments.clone();
                            let count = module.split("::").count();
                            bound.segments = module
                                .split("::")
                                .enumerate()
                                .map(|(index, text)| Segment {
                                    name: crate::item::Name {
                                        text: text.into(),
                                        span: at,
                                    },
                                    arguments: if index + 1 == count {
                                        written.clone()
                                    } else {
                                        Box::default()
                                    },
                                    span,
                                })
                                .chain(bound.segments.iter().skip(1).cloned())
                                .collect();
                        } else {
                            qualify(argument, modules);
                        }
                    }
                }
            }
            Type::Reference { target, .. } => qualify(target, modules),
            Type::Tuple { members, .. } => {
                for member in members.iter_mut() {
                    qualify(member, modules);
                }
            }
            Type::Unit(_) => {}
        }
    }
    // `impl canvas::Program<Message> for Arc`: a dependency's trait by a
    // module the unit's `use`s bind, written from the crate's root, so the
    // impl's header names the trait as the registry does.
    if let crate::item::Parsed::Impl(item) = item
        && let Some(path) = &mut item.trait_
        && !path.global
        && let Some(module) = path
            .segments
            .first()
            .and_then(|first| modules.get(&first.name.text))
    {
        let (at, span) = (path.segments[0].name.span, path.segments[0].span);
        let written = path.segments[0].arguments.clone();
        let single = path.segments.len() == 1;
        let count = module.split("::").count();
        path.segments = module
            .split("::")
            .enumerate()
            .map(|(index, text)| Segment {
                name: crate::item::Name {
                    text: text.into(),
                    span: at,
                },
                arguments: if single && index + 1 == count {
                    written.clone()
                } else {
                    Box::default()
                },
                span,
            })
            .chain(path.segments.iter().skip(1).cloned())
            .collect();
    }
    // `impl<R> Widget<M, T, R> for Quad where R: renderer::Renderer`: the
    // impl's own bounds name a dependency's trait the same way (D629).
    if let crate::item::Parsed::Impl(item) = item {
        qualify_bounds(&mut item.generics, modules);
    }
    let functions: Vec<&mut crate::item::Function> = match item {
        crate::item::Parsed::Function(function) => vec![function],
        crate::item::Parsed::Impl(item) => item.functions.iter_mut().collect(),
        _ => Vec::new(),
    };
    for function in functions {
        if let Some(output) = &mut function.output {
            qualify(output, modules);
        }
        // `fn modal(base: impl Widget<Message>)`, whose parameter's `impl`
        // is lifted to a generic parameter bounded by `Widget<Message>`,
        // or `W: Widget<Message>`: a dependency's trait by a module the
        // unit's `use`s bind, written from the crate's root (D629).
        qualify_bounds(&mut function.generics, modules);
    }
}

/// Each path of `item`'s signatures that starts at a module a `use` binds
/// under another name, as `_serde::Serialize` after `extern crate serde as
/// _serde;` or `s::fmt::Formatter` after `use std as s;`, written from the
/// module's own path, as rustc reads it through the binding.
pub(super) fn write_renamed_modules(
    item: &mut crate::item::Parsed,
    modules: &BTreeMap<ImmutableString, ImmutableString>,
) {
    use crate::item::type_ref::{Path, Segment};
    let renamed = modules
        .iter()
        .filter(|(name, qualified)| qualified.rsplit("::").next() != Some(name.as_str()))
        .collect::<BTreeMap<_, _>>();
    if renamed.is_empty() {
        return;
    }
    let path = |path: &mut Path| {
        if path.global {
            return;
        }
        let Some(module) = path
            .segments
            .first()
            .filter(|first| first.arguments.is_empty())
            .and_then(|first| renamed.get(&first.name.text))
        else {
            return;
        };
        let (at, span) = (path.segments[0].name.span, path.segments[0].span);
        path.segments = module
            .split("::")
            .map(|text| Segment {
                name: crate::item::Name {
                    text: text.into(),
                    span: at,
                },
                arguments: Box::default(),
                span,
            })
            .chain(path.segments.iter().skip(1).cloned())
            .collect();
    };
    each_signature_path(item, &path);
}

/// Call `path` on each path of `item`'s signatures: its types, at any depth
/// of their arguments, its generic bounds and an impl's trait.
fn each_signature_path(
    item: &mut crate::item::Parsed,
    path: &impl Fn(&mut crate::item::type_ref::Path),
) {
    use crate::item::type_ref::{Argument, Path, Type};
    fn ty(written: &mut Type, path: &impl Fn(&mut Path)) {
        match written {
            Type::Unit(_) => {}
            Type::Tuple { members, .. } => {
                for member in members.iter_mut() {
                    ty(member, path);
                }
            }
            Type::Path(written) => {
                path(written);
                for segment in written.segments.iter_mut() {
                    for argument in segment.arguments.iter_mut() {
                        if let Argument::Type(argument) = argument {
                            ty(argument, path);
                        }
                    }
                }
            }
            Type::Reference { target, .. } => ty(target, path),
        }
    }
    let generics = |generics: &mut [crate::item::GenericParam]| {
        for generic in generics.iter_mut() {
            let mut bounds = generic.bounds.to_vec();
            for bound in &mut bounds {
                path(bound);
            }
            generic.bounds = bounds.into_boxed_slice();
        }
    };
    let function = |function: &mut crate::item::Function| {
        generics(&mut function.generics);
        for parameter in function.parameters.iter_mut() {
            ty(&mut parameter.ty, path);
        }
        if let Some(output) = &mut function.output {
            ty(output, path);
        }
    };
    match item {
        crate::item::Parsed::Struct(item) => {
            generics(&mut item.generics);
            for field in item.fields.iter_mut() {
                ty(&mut field.ty, path);
            }
        }
        crate::item::Parsed::Enum(item) => {
            generics(&mut item.generics);
            for variant in item.variants.iter_mut() {
                for payload in variant.payload.iter_mut() {
                    ty(payload, path);
                }
            }
        }
        crate::item::Parsed::Impl(item) => {
            generics(&mut item.generics);
            ty(&mut item.owner, path);
            if let Some(trait_) = &mut item.trait_ {
                path(trait_);
            }
            for member in item.functions.iter_mut() {
                function(member);
            }
        }
        crate::item::Parsed::Trait(item) => {
            for member in item.functions.iter_mut() {
                function(member);
            }
        }
        crate::item::Parsed::Function(item) => function(item),
        crate::item::Parsed::TypeAlias(item) => ty(&mut item.target, path),
        crate::item::Parsed::Module(_) | crate::item::Parsed::Use(_) => {}
    }
}

/// Each bound of `generics` that names a dependency's trait by a module the
/// unit's `use`s bind (`Renderer: renderer::Renderer`), written from the
/// crate's root.
fn qualify_bounds(
    generics: &mut [crate::item::GenericParam],
    modules: &BTreeMap<ImmutableString, ImmutableString>,
) {
    use crate::item::type_ref::Segment;
    for generic in generics.iter_mut() {
        let mut bounds = generic.bounds.to_vec();
        for bound in &mut bounds {
            if bound.global {
                continue;
            }
            let Some(module) = bound
                .segments
                .first()
                .and_then(|first| modules.get(&first.name.text))
            else {
                continue;
            };
            let (at, span) = (bound.segments[0].name.span, bound.segments[0].span);
            let written = bound.segments[0].arguments.clone();
            let single = bound.segments.len() == 1;
            let count = module.split("::").count();
            bound.segments = module
                .split("::")
                .enumerate()
                .map(|(index, text)| Segment {
                    name: crate::item::Name {
                        text: text.into(),
                        span: at,
                    },
                    arguments: if single && index + 1 == count {
                        written.clone()
                    } else {
                        Box::default()
                    },
                    span,
                })
                .chain(bound.segments.iter().skip(1).cloned())
                .collect();
        }
        generic.bounds = bounds.into_boxed_slice();
    }
}

pub(super) fn collect_item_type_paths<'a>(
    item: &'a crate::item::Parsed,
    output: &mut Vec<&'a crate::item::type_ref::Path>,
) {
    match item {
        crate::item::Parsed::Struct(item) => {
            output.extend(
                item.generics
                    .iter()
                    .flat_map(|generic| generic.bounds.iter()),
            );
            for field in &item.fields {
                collect_type_paths(&field.ty, output);
            }
        }
        crate::item::Parsed::Enum(item) => {
            output.extend(
                item.generics
                    .iter()
                    .flat_map(|generic| generic.bounds.iter()),
            );
            for variant in &item.variants {
                for ty in &variant.payload {
                    collect_type_paths(ty, output);
                }
            }
        }
        crate::item::Parsed::Impl(item) => {
            collect_type_paths(&item.owner, output);
            // `impl component::Component<..> for C`, or a bound's
            // `renderer::Renderer`: a facade trait by its module.
            output.extend(item.trait_.iter());
            output.extend(
                item.generics
                    .iter()
                    .flat_map(|generic| generic.bounds.iter()),
            );
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
        crate::item::Parsed::TypeAlias(item) => collect_type_paths(&item.target, output),
        crate::item::Parsed::Module(_) | crate::item::Parsed::Use(_) => {}
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
    host: &crate::host::Registry,
    expanded: &crate::capability::Expanded,
) -> Result<Vec<type_def::Definition>, LocatedError> {
    let mut schemas = BTreeMap::new();
    let capabilities = crate::capability::resolve(items, host, expanded)?;
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
