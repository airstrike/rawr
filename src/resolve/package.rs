//! Aggregate semantic resolution for a validated static package graph.

mod kept;
mod tables;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) use kept::Resolutions;
use tables::Tables;

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

/// The type each pattern names through a module, by where its path is.
type PatternOwners = BTreeMap<(program::source::Id, Span), type_def::Identity>;

/// The items each block of a unit declares, by name, with the spans of
/// their names: no path names them, and only their block sees them
/// (`super::block`).
type BlockItems = BTreeMap<Span, BTreeMap<ImmutableString, (Span, Target)>>;

/// The modules each block of a unit declares, by block, each name with
/// where it is declared and its module's segment
/// (`super::block::module_segment`).
type BlockModules = BTreeMap<Span, BTreeMap<ImmutableString, (Span, ImmutableString)>>;

/// The qualified paths of items' headers one pass resolves: those that
/// start at a module's own names, which the module's names hold, or those
/// through a block's module (`Resolver::in_blocks`), which that block's
/// names hold.
#[derive(Clone, Copy, PartialEq)]
enum HeaderPaths {
    Module,
    Block,
}

/// Each name of Rust's namespaces a scope binds, with where, and whether an
/// import bound it: two imports are rustc's E0252.
type Bound =
    BTreeMap<(ImmutableString, item::Namespace), (source::Id, Span, Option<super::Import>)>;

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

#[derive(Clone, PartialEq)]
struct Declaration {
    source: source::Id,
    span: Span,
    reach: super::Reach,
    target: Target,
}

#[derive(Clone, PartialEq)]
enum Target {
    Module,
    /// A crate trait at its address, known by its name, or by its path
    /// when another module's trait or a std trait has that name
    /// ([`Resolver::trait_names`]). A signature names it through its
    /// module's scope ([`Resolver::write_trait_names`]).
    Trait(Address),
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
            Self::Module | Self::Trait(_) | Self::Type(_) => item::Namespace::Type,
            Self::Item(_) | Self::HostItem { .. } => item::Namespace::Value,
            Self::HostModuleItem { .. } => item::Namespace::Both,
        }
    }
}

#[derive(Clone, Default)]
struct ResolvedQualifiedCalls {
    functions: BTreeMap<(program::source::Id, Span), crate::item::Key>,
    owners: BTreeMap<(program::source::Id, Span), type_def::Identity>,
    aliases: BTreeMap<(program::source::Id, Span), type_ref::Alias>,
    rust: BTreeMap<(program::source::Id, Span), Box<[ImmutableString]>>,
    traits: BTreeMap<(program::source::Id, Span), ImmutableString>,
}

struct QualifiedCall {
    path: Box<[ImmutableString]>,
    path_span: Span,
    owner_span: Span,
    /// The last segment, the item the path names in its owner.
    name_span: Span,
}

#[derive(Clone, PartialEq)]
struct Export {
    source: source::Id,
    context: Context,
    path: Box<[ImmutableString]>,
    span: Span,
    /// Where the binding is written, its path and its `as` name: what
    /// rustc shows of an import.
    written: Span,
    /// Where the binding is visible, as its `use`'s `pub` marker says.
    reach: super::Reach,
}

#[derive(Clone, PartialEq)]
struct Context {
    source: source::Id,
    package: graph::Id,
    namespace: Namespace,
}

struct Unit {
    context: Context,
    parsed: parse::Unit,
    slots: Vec<Option<Slot>>,
    blocks: BlockItems,
}

#[derive(Clone)]
struct Slot {
    id: crate::item::Id,
    key: Option<crate::item::Key>,
}

struct Resolver<'a> {
    #[cfg(feature = "rust-import")]
    native: Option<std::rc::Rc<super::native::Capture>>,
    host: &'a host::Registry,
    package: ImmutableString,
    evidence: Box<[crate::diagnostic::Source]>,
    units: Vec<Unit>,
    entry_source: source::Id,
    /// A host's one in-memory file, which has no extern prelude to bound
    /// its paths ([`Graph::hosted`]).
    hosted: bool,
    /// The names the crate's modules bind, and its packages.
    tables: Tables,
    /// What the compiler keeps of its resolutions, when it keeps them
    /// (#1876).
    kept: Option<&'a RefCell<Resolutions>>,
    type_declarations: Vec<type_def::Declaration>,
    /// The modules the blocks of each unit that has one declare: a
    /// block's module is named in its block only.
    block_modules: BTreeMap<source::Id, BlockModules>,
    /// The expansions each source lays out past its file's lines, whose
    /// paths are read in the blocks around their calls (`macros::sites`).
    expansions: BTreeMap<source::Id, Box<[crate::diagnostic::source::Expansion]>>,
    /// The ids `type_declarations` hold (`type_def::Id::keyed`).
    type_ids: BTreeSet<type_def::Id>,
    /// Each crate trait whose name another module's trait or a std trait
    /// has too, by its address, with the path it is known by, as rustc
    /// names it then: `home::IntoView` beside `panel::IntoView`, and
    /// `m::Display` beside std's. A trait whose name is the crate's only
    /// one is known by its name.
    trait_names: BTreeMap<Address, ImmutableString>,
    /// Each name a signature may write a crate trait by: every crate
    /// trait's own name, and each name a `use` binds with `as`
    /// ([`Resolver::write_trait_names`]).
    trait_spellings: BTreeSet<ImmutableString>,
    /// Each crate derive whose expansion the graph loaded (#593).
    expanded: crate::capability::Expanded,
    /// The marked types of a native root that rawr's copy of the crate
    /// mirrors, by their package and path, each with its Rust path from
    /// the crate's name (`Graph::island_mirrors`, #1249).
    #[cfg(feature = "rust-import")]
    mirrors: BTreeMap<Box<[ImmutableString]>, Box<[ImmutableString]>>,
    /// The roots of the facades of the program's own crate and its
    /// binary's (`Program::own`).
    #[cfg(feature = "rust-import")]
    own: Box<[ImmutableString]>,
    /// The instances native code calls marked generic functions at
    /// (`Graph::native_instances`, D869).
    #[cfg(all(feature = "rust-import", not(target_arch = "wasm32")))]
    natives: Vec<rawr_import::record::Instance>,
}

impl Resolver<'_> {
    /// The program's own types and traits named `name`, where it is
    /// `wanted` as a type, that another module of the package declares and
    /// opens to `context`, each by its path from the crate's root. Its
    /// values are the lowering's (`lower_nominal::names`).
    fn own_candidates(
        &self,
        context: &Context,
        name: &str,
        wanted: super::imports::Wanted,
    ) -> Vec<super::Candidate> {
        use super::imports::Wanted;
        if !matches!(wanted, Wanted::Type | Wanted::Trait) {
            return Vec::new();
        }
        let namespace = &context.namespace;
        self.tables
            .addressed()
            .filter(|(address, declaration)| {
                address.name == name
                    && address.package == namespace.package
                    && address.modules != namespace.modules
                    && declaration.reach.reaches(namespace)
            })
            .filter_map(|(address, declaration)| {
                let noun = match &declaration.target {
                    Target::Trait(_) => "trait",
                    _ if wanted == Wanted::Trait => return None,
                    Target::Type(type_ref::Target::Script { definition, .. }) => {
                        match self
                            .type_declarations
                            .iter()
                            .find(|declared| declared.id() == *definition)?
                            .kind()
                        {
                            type_def::Kind::Struct => "struct",
                            type_def::Kind::Enum => "enum",
                        }
                    }
                    Target::Type(type_ref::Target::Alias(_)) => "type alias",
                    _ => return None,
                };
                let path = std::iter::once("crate")
                    .chain(address.modules.iter().map(ImmutableString::as_str))
                    .chain([name])
                    .collect::<Vec<_>>()
                    .join("::");
                Some(super::Candidate {
                    path: path.into(),
                    noun,
                })
            })
            .collect()
    }
}

/// The variants of `std::cmp::Ordering`, which `use` may import by name.
pub(crate) const ORDERING_VARIANTS: [&str; 3] = ["Less", "Equal", "Greater"];

pub(crate) fn resolve(graph: Graph, host: &host::Registry) -> Result<Program, Failure> {
    Resolver::new(
        graph,
        host,
        #[cfg(feature = "rust-import")]
        None,
        None,
    )?
    .resolve()
}

#[cfg(feature = "rust-import")]
pub(crate) fn resolve_captured(
    graph: Graph,
    host: &host::Registry,
    native: std::rc::Rc<super::native::Capture>,
) -> Result<Program, Failure> {
    Resolver::new(graph, host, Some(native), None)?.resolve()
}

/// [`resolve_captured`], reusing each unit's resolution that `kept` holds
/// while what it reads is as it was, and keeping each one it resolves
/// again (#1876). `registry` is the engine's registry, `host`, with how
/// many times it was changed.
#[cfg(feature = "rust-import")]
pub(crate) fn resolve_kept(
    graph: Graph,
    registry: &host::Current,
    native: Option<std::rc::Rc<super::native::Capture>>,
    kept: &mut Resolutions,
) -> Result<Program, Failure> {
    let cell = RefCell::new(std::mem::take(kept));
    let resolved = Resolver::new(graph, registry, native, Some((&cell, registry.changes())))
        .and_then(Resolver::resolve);
    *kept = cell.into_inner();
    resolved
}

impl<'a> Resolver<'a> {
    /// The resolver of `graph`, its units declared and their globs bound,
    /// which records misses against native code in `native`. One that
    /// keeps its resolutions in `kept` reads what the last one kept there,
    /// which read the registry as it was after so many changes (#1876).
    fn new(
        graph: Graph,
        host: &'a host::Registry,
        #[cfg(feature = "rust-import")] native: Option<std::rc::Rc<super::native::Capture>>,
        kept: Option<(&'a RefCell<Resolutions>, u64)>,
    ) -> Result<Self, Failure> {
        #[cfg(feature = "rust-import")]
        let roots = native.as_ref().map(|capture| capture.roots().into());
        #[cfg(not(feature = "rust-import"))]
        let roots = None;
        let expanded = graph.expanded();
        let hosted = graph.hosted();
        #[cfg(feature = "rust-import")]
        let mirrors = graph.island_mirrors();
        #[cfg(feature = "rust-import")]
        let statics = graph.native_statics();
        #[cfg(all(feature = "rust-import", not(target_arch = "wasm32")))]
        let natives = graph.native_instances();
        let (packages, sources, parsed, entry_package, entry_source) = graph.into_parts();
        let package: ImmutableString = packages[entry_package.index()].name().into();
        // The facades of the program's own crate and its binary's, which
        // rawr's copy of the crate is built as.
        #[cfg(feature = "rust-import")]
        let own = {
            let entry = &packages[entry_package.index()];
            entry
                .rust_crate()
                .map(ImmutableString::from)
                .into_iter()
                .chain(
                    entry
                        .bin()
                        .is_some()
                        .then(|| ImmutableString::from(crate::package::island::BIN)),
                )
                .collect()
        };
        // Each mirror by its package's path, and by its Rust path from the
        // crate's name, as rawr's copy of the crate declares it.
        #[cfg(feature = "rust-import")]
        let mirrors = {
            let krate = packages[entry_package.index()]
                .rust_crate()
                .map_or_else(|| package.clone(), ImmutableString::from);
            mirrors
                .into_iter()
                .map(|path| {
                    let rust = std::iter::once(krate.clone())
                        .chain(path.iter().cloned())
                        .collect();
                    (std::iter::once(package.clone()).chain(path).collect(), rust)
                })
                .collect()
        };
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
                    blocks: BlockItems::new(),
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

        // The crates this package's paths may start with, as rustc's extern
        // prelude has them. None for a host's one in-memory file, whose host
        // decides what it reaches. The entry package's root is the project
        // the program is built in.
        let project = packages[entry_package.index()].root();
        let prelude = packages
            .iter()
            .filter(|_| !hosted)
            .find(|candidate| candidate.name() == package.as_str())
            .map(|package| super::extern_prelude::Prelude::of(package, project));
        let mut resolver = Self {
            #[cfg(feature = "rust-import")]
            native: None,
            host,
            package,
            evidence,
            units,
            entry_source,
            hosted,
            tables: Tables::new(packages, prelude),
            kept: kept.map(|(kept, _)| kept),
            type_declarations: Vec::new(),
            block_modules: BTreeMap::new(),
            expansions: BTreeMap::new(),
            type_ids: BTreeSet::new(),
            trait_names: BTreeMap::new(),
            trait_spellings: BTreeSet::new(),
            expanded,
            #[cfg(feature = "rust-import")]
            mirrors,
            #[cfg(feature = "rust-import")]
            own,
            #[cfg(all(feature = "rust-import", not(target_arch = "wasm32")))]
            natives,
        };
        resolver.predeclare()?;
        resolver.trait_names = resolver.shared_trait_names();
        resolver.trait_spellings = resolver.trait_spellings();
        if let Some((kept, changes)) = kept {
            kept.borrow_mut().refresh(&resolver, changes, roots);
        }
        resolver.predeclare_globs();
        #[cfg(feature = "rust-import")]
        {
            resolver.native = native;
        }
        Ok(resolver)
    }

    /// Declare each unit's items and imports. Each unit's items are read
    /// where the unit holds them, never copied (#1850).
    fn predeclare(&mut self) -> Result<(), Failure> {
        let mut units = std::mem::take(&mut self.units);
        let declared = self.predeclare_units(&mut units);
        self.units = units;
        declared
    }

    fn predeclare_units(&mut self, units: &mut [Unit]) -> Result<(), Failure> {
        let mut next_item = 0_usize;

        for unit in units {
            let context = &unit.context;
            let items = unit.parsed.items();
            let mut slots = Vec::with_capacity(items.len());
            let mut blocks = BlockItems::new();
            let mut block_modules = BlockModules::new();
            let ordinals = super::block::ordinals(items);

            for (index, parsed) in items.iter().enumerate() {
                if let crate::item::Parsed::Module(module) = parsed {
                    let segment = super::block::module_segment(&module.name.text, ordinals[index]);
                    // Each block's modules are named apart, so a second
                    // module of one name in one block is rustc's E0428.
                    if let Some(block) = parsed.scope()
                        && let Some((previous, _)) = block_modules.entry(block).or_default().insert(
                            module.name.text.clone(),
                            (module.name.span, segment.clone()),
                        )
                    {
                        return Err(Failure {
                            primary: context.source,
                            secondary: Some(context.source),
                            error: Error::DuplicateItem {
                                name: module.name.text.clone(),
                                declaration: module.name.span,
                                previous_declaration: previous,
                            },
                        });
                    }
                    let address = context.address(&segment);
                    self.insert_declaration(
                        address.clone(),
                        Declaration {
                            source: context.source,
                            span: module.name.span,
                            reach: context.namespace.reach(module.header.visibility.as_ref()),
                            target: Target::Module,
                        },
                    )?;
                    slots.push(None);
                    continue;
                }

                if let crate::item::Parsed::TypeAlias(alias) = parsed {
                    let target = Target::Type(super::unresolved_alias(alias));
                    match parsed.scope() {
                        Some(block) => {
                            declare_in_block(&mut blocks, block, context, &alias.name, target)?;
                        }
                        None => self.insert_declaration(
                            context.address(&alias.name.text),
                            Declaration {
                                source: context.source,
                                span: alias.name.span,
                                reach: context.namespace.reach(alias.header.visibility.as_ref()),
                                target,
                            },
                        )?,
                    }
                    slots.push(None);
                    continue;
                }
                if let crate::item::Parsed::Trait(declared) = parsed {
                    let address = context.address(&declared.name.text);
                    self.insert_declaration(
                        address.clone(),
                        Declaration {
                            source: context.source,
                            span: declared.name.span,
                            reach: context.namespace.reach(declared.header.visibility.as_ref()),
                            target: Target::Trait(address),
                        },
                    )?;
                }
                let id = crate::item::Id::from_index(next_item);
                next_item += 1;
                let mut slot = Slot { id, key: None };

                if let Some((name, type_kind, item_kind)) = super::named_declaration(parsed) {
                    let ordinal = ordinals[index];
                    let key = context
                        .namespace
                        .item_key(&name.text, item_kind)
                        .in_block(ordinal);
                    slot.key = Some(key.clone());
                    let target = match type_kind {
                        Some(kind) => {
                            let type_key = context.namespace.type_key(&name.text).in_block(ordinal);
                            let definition = type_def::Id::keyed(&type_key, &mut self.type_ids);
                            if let crate::item::Parsed::Enum(declared) = parsed {
                                self.tables.declare_variants(
                                    definition,
                                    declared
                                        .variants
                                        .iter()
                                        .map(|variant| variant.name.text.clone())
                                        .collect(),
                                );
                            }
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
                    match parsed.scope() {
                        Some(block) => declare_in_block(&mut blocks, block, context, name, target)?,
                        None => self.insert_declaration(
                            context.address(&name.text),
                            Declaration {
                                source: context.source,
                                span: name.span,
                                reach: context.namespace.reach(
                                    header(parsed).and_then(|header| header.visibility.as_ref()),
                                ),
                                target,
                            },
                        )?,
                    }
                }
                slots.push(Some(slot));
            }

            for parsed in items {
                let crate::item::Parsed::Use(use_item) = parsed else {
                    continue;
                };
                // A block's `use` binds its names in its block only.
                if use_item.header.scope.is_some() {
                    continue;
                }
                if use_item.header.visibility.is_none() {
                    for (name, path) in use_paths(use_item) {
                        let address = context.address(&name.text);
                        let export = Export {
                            source: context.source,
                            context: context.clone(),
                            path,
                            span: name.span,
                            written: use_item.written(&name),
                            reach: context.namespace.reach(None),
                        };
                        self.tables.alias_at(address, export);
                    }
                    continue;
                }
                for (name, path) in use_paths(use_item) {
                    let address = context.address(&name.text);
                    // rustc's E0255, in the declaration's namespace; the
                    // import binds there what the declaration is.
                    if let Some(previous) = self.tables.declaration(&address) {
                        let (namespace, kind) = match previous.target {
                            Target::Module => ("type", "module"),
                            Target::Trait(_) => ("type", "trait"),
                            Target::Item(_) => ("value", "value"),
                            _ => ("type", "type"),
                        };
                        let import = super::Import {
                            written: use_item.written(&name),
                            rename: use_item.rename(&name),
                            kind,
                        };
                        return Err(Failure {
                            primary: context.source,
                            secondary: Some(previous.source),
                            error: Error::duplicate(
                                name.text,
                                namespace,
                                import,
                                (previous.span, None),
                                previous.source == context.source,
                            ),
                        });
                    }
                    if let Some(previous) = self.tables.export_at(
                        address,
                        Export {
                            source: context.source,
                            context: context.clone(),
                            path,
                            span: name.span,
                            written: use_item.written(&name),
                            reach: context.namespace.reach(use_item.header.visibility.as_ref()),
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

            if !unit.parsed.expansions().is_empty() {
                self.expansions
                    .insert(unit.context.source, unit.parsed.expansions().into());
            }
            if !block_modules.is_empty() {
                self.block_modules
                    .insert(unit.context.source, block_modules);
            }
            unit.slots = slots;
            unit.blocks = blocks;
        }
        Ok(())
    }

    /// Each trait whose name another declared trait or a std trait has,
    /// with its path from its crate's root, its package's name first
    /// outside the entry package: a module's `Display` is `m::Display`, as
    /// rustc names it, and the bare name a scope without it writes is
    /// std's.
    fn shared_trait_names(&self) -> BTreeMap<Address, ImmutableString> {
        let mut by_name = BTreeMap::<&str, Vec<&Address>>::new();
        for declaration in self.tables.declarations() {
            if let Target::Trait(address) = &declaration.target {
                by_name
                    .entry(address.name.as_str())
                    .or_default()
                    .push(address);
            }
        }
        by_name
            .into_iter()
            .filter(|(name, addresses)| {
                addresses.len() > 1 || super::std_trait::anywhere(name).is_some()
            })
            .flat_map(|(_, addresses)| addresses)
            .map(|address| {
                let package = (address.package != self.package).then_some(&address.package);
                let path = package
                    .into_iter()
                    .chain(address.modules.iter())
                    .chain(std::iter::once(&address.name))
                    .map(ImmutableString::as_str)
                    .collect::<Vec<_>>()
                    .join("::");
                (address.clone(), path.into())
            })
            .collect()
    }

    /// Each name a signature may write a crate trait by
    /// ([`Resolver::trait_spellings`]).
    fn trait_spellings(&self) -> BTreeSet<ImmutableString> {
        let declared =
            self.tables
                .declarations()
                .filter_map(|declaration| match &declaration.target {
                    Target::Trait(address) => Some(address.name.clone()),
                    _ => None,
                });
        let renamed = self
            .units
            .iter()
            .flat_map(|unit| unit.parsed.items())
            .filter_map(|parsed| match parsed {
                crate::item::Parsed::Use(used) => Some(used.bindings()),
                _ => None,
            })
            .flatten()
            .filter(|(name, path)| path.last() != Some(&name.text))
            .map(|(name, _)| name.text);
        declared.chain(renamed).collect()
    }

    /// Bind each glob import's names, a public glob's as re-exports, until
    /// none binds more: a glob
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
                    // `pub use Shade::*;` re-exports the enum's variants,
                    // and `pub use inner::*;` the module's names (#1349).
                    let public = used.header.visibility.is_some();
                    for name in self.glob_names(context, &segments).unwrap_or_default() {
                        let address = context.address(&name);
                        if self.tables.binds(&address) || found.contains_key(&address) {
                            continue;
                        }
                        let mut path = segments.to_vec();
                        path.push(name);
                        // A glob re-exports each name no further than
                        // the item reaches: `pub use inner::*;` of a
                        // `pub(super) fn` binds it for the parent alone.
                        let mut reach = context.namespace.reach(used.header.visibility.as_ref());
                        let (origin, origin_path) = self.origin(context, &path);
                        if let Some((_, declaration)) = self.declared_at(&origin, &origin_path)
                            && !reach.within(&declaration.reach)
                        {
                            reach = declaration.reach.clone();
                        }
                        let export = Export {
                            source: context.source,
                            context: context.clone(),
                            path: path.into_boxed_slice(),
                            span: *span,
                            written: *span,
                            reach,
                        };
                        found.insert(address, (export, public));
                    }
                }
            }
            if found.is_empty() {
                return;
            }
            for (address, (export, public)) in found {
                self.tables.glob_at(address, export, public);
            }
        }
    }

    fn insert_declaration(
        &mut self,
        address: Address,
        declaration: Declaration,
    ) -> Result<(), Failure> {
        if let Some(previous) = self.tables.declare(address.clone(), declaration.clone()) {
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
            let found = self.reused(
                unit.context.source,
                kept::Unit::calls,
                |_| true,
                || self.unit_qualified_calls(unit),
            )?;
            resolved.functions.extend(found.functions);
            resolved.owners.extend(found.owners);
            resolved.aliases.extend(found.aliases);
            resolved.rust.extend(found.rust);
            resolved.traits.extend(found.traits);
        }
        Ok(resolved)
    }

    /// The qualified paths `unit`'s bodies call or name, by where they are.
    fn unit_qualified_calls(&self, unit: &Unit) -> Result<ResolvedQualifiedCalls, Failure> {
        let mut resolved = ResolvedQualifiedCalls::default();
        let mut statements = None;
        for body in unit_bodies(&unit.parsed, &mut statements) {
            for call in qualified_calls(body) {
                let scoped = self.in_blocks(&unit.context, &call.path, call.path_span);
                let path = scoped.as_deref().unwrap_or(&call.path);
                if !self.is_script_path(&unit.context, path) {
                    continue;
                }
                let source = program::source::Id::from_index(unit.context.source.index());
                // `crate::model::Thing::new` into a `.rs` module
                // is the app crate's `polyglot::model::Thing::new`.
                if let Some(spelled) = self.rust_spelling(&unit.context, path) {
                    resolved
                        .rust
                        .insert((source, call.path_span), spelled.into_boxed_slice());
                    continue;
                }
                // `kinds::Secondary` after `pub use self::Kind::Secondary;`
                // in `kinds`: the variant of the enum the import names.
                let (origin_context, origin_path) = self.origin(&unit.context, path);
                if *origin_path != *path {
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
                                .chain(path.last().cloned())
                                .collect();
                            resolved.rust.insert((source, call.path_span), spelled);
                            continue;
                        }
                        _ => {}
                    }
                }

                if path.len() >= 3 {
                    let mut active = BTreeSet::new();
                    match self
                        .resolve_path(
                            &unit.context,
                            &path[..path.len() - 1],
                            call.owner_span,
                            &mut active,
                        )
                        .map_err(|failure| {
                            self.missing_segment(
                                &unit.context,
                                path,
                                |index| written_segment(path, call.path_span, index),
                                failure,
                            )
                        })
                        .map_err(|failure| self.through_private(&unit.context, &call, failure))?
                    {
                        Target::Type(type_ref::Target::Script { identity, .. }) => {
                            resolved.owners.insert((source, call.owner_span), identity);
                            continue;
                        }
                        // `home::Shape::size(&x)`: the trait's function.
                        Target::Trait(address) => {
                            let known = self.known_trait_name(address);
                            resolved.traits.insert((source, call.owner_span), known);
                            continue;
                        }
                        Target::Module | Target::HostModuleItem { .. } | Target::Item(_) => {}
                        Target::HostItem { .. }
                        | Target::Type(
                            type_ref::Target::Builtin
                            | type_ref::Target::Host { .. }
                            | type_ref::Target::Parameter
                            | type_ref::Target::Opaque(_)
                            | type_ref::Target::Projection(..)
                            | type_ref::Target::Alias(_)
                            | type_ref::Target::Std(_),
                        ) => continue,
                    }
                }

                let mut active = BTreeSet::new();
                match self
                    .resolve_path(&unit.context, path, call.path_span, &mut active)
                    .map_err(|failure| self.not_in_module(&unit.context, &call, failure))
                    .map_err(|failure| {
                        self.missing_segment(
                            &unit.context,
                            path,
                            |index| written_segment(path, call.path_span, index),
                            failure,
                        )
                    })
                    .map_err(|failure| self.through_private(&unit.context, &call, failure))
                    .map_err(|failure| self.private_value(&unit.context, &call, failure))?
                {
                    Target::Item(key) => {
                        resolved.functions.insert((source, call.path_span), key);
                    }
                    // A qualified type, as in a `let` annotation.
                    Target::Type(type_ref::Target::Script { identity, .. }) => {
                        resolved.owners.insert((source, call.path_span), identity);
                    }
                    // A qualified type alias, as `m::Mask` in a `let`
                    // annotation: the alias its module declares.
                    Target::Type(type_ref::Target::Alias(alias)) => {
                        resolved.aliases.insert((source, call.path_span), alias);
                    }
                    // `<T as home::Shape>::size(..)`'s trait.
                    Target::Trait(address) => {
                        let known = self.known_trait_name(address);
                        resolved.traits.insert((source, call.path_span), known);
                    }
                    _ => {}
                }
            }
        }
        Ok(resolved)
    }

    fn resolve_qualified_pattern_owners(&self) -> Result<PatternOwners, Failure> {
        let mut resolved = BTreeMap::new();
        for unit in &self.units {
            let found = self.reused(
                unit.context.source,
                kept::Unit::owners,
                |_| true,
                || self.unit_pattern_owners(unit),
            )?;
            resolved.extend(found);
        }
        Ok(resolved)
    }

    /// The types `unit`'s patterns name through a module, by where they
    /// are.
    fn unit_pattern_owners(&self, unit: &Unit) -> Result<PatternOwners, Failure> {
        let mut resolved = BTreeMap::new();
        let mut statements = None;
        for body in unit_bodies(&unit.parsed, &mut statements) {
            for (path, owner) in qualified_pattern_owners(body) {
                let scoped = self.in_blocks(&unit.context, &path, owner);
                let path = scoped.as_deref().unwrap_or(&path);
                if !self.is_script_path(&unit.context, path) {
                    continue;
                }
                let mut active = BTreeSet::new();
                match self.resolve_path(&unit.context, path, owner, &mut active)? {
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
                    | Target::Trait(_)
                    | Target::HostModuleItem { .. }
                    | Target::Item(_)
                    | Target::HostItem { .. }
                    | Target::Type(
                        type_ref::Target::Builtin
                        | type_ref::Target::Host { .. }
                        | type_ref::Target::Parameter
                        | type_ref::Target::Opaque(_)
                        | type_ref::Target::Projection(..)
                        | type_ref::Target::Alias(_)
                        | type_ref::Target::Std(_),
                    ) => {}
                }
            }
        }
        Ok(resolved)
    }

    fn resolve(mut self) -> Result<Program, Failure> {
        super::std_trait::begin();
        self.validate_restrictions()?;
        self.validate_exports()?;
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
                    expansions: unit.parsed.expansions(),
                }),
        );

        self.resolve_aliases()?;
        // After the aliases, whose targets a qualified path to one reads.
        let qualified_calls = self.resolve_qualified_calls()?;
        let trait_names = self.source_trait_names();
        let prelude = self.prelude();
        let mut aliases = BTreeMap::new();
        let mut lexical = BTreeMap::new();
        let mut placements = BTreeMap::new();
        // Each unit's context, whose names an error past its own
        // resolution, as a trait an impl names, offers to import.
        let mut contexts = BTreeMap::new();
        let units = std::mem::take(&mut self.units);
        // Where each derive's expansion is, in the source of its item, and
        // the names the blocks around it bind there, which the expansion
        // sees as its item does (#1631).
        let derives = self.derive_sites();
        let mut around = BTreeMap::<source::Id, BTreeMap<ImmutableString, type_ref::Target>>::new();
        // The crate root's names, which a recorded instance's types are
        // written in (D869).
        #[cfg(all(feature = "rust-import", not(target_arch = "wasm32")))]
        let mut root_types = None;
        for Unit {
            context,
            parsed,
            slots,
            blocks,
        } in units
        {
            lexical.insert(
                program::source::Id::from_index(context.source.index()),
                parsed.imports().to_vec().into_boxed_slice(),
            );
            contexts.insert(context.source, context.clone());
            let placement = parsed.use_placement();
            if let Some(at) = placement {
                placements.insert(program::source::Id::from_index(context.source.index()), at);
            }
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

            let entering = around.remove(&context.source).unwrap_or_default();
            let sites = derives
                .iter()
                .filter(|(_, (item, _))| *item == context.source)
                .map(|(expansion, (_, derive))| (*expansion, *derive))
                .collect::<Vec<_>>();
            let resolved = self
                .reused(
                    context.source,
                    kept::Unit::resolved,
                    |kept| kept.entering == entering && kept.sites == sites,
                    || {
                        self.resolve_unit(
                            &context,
                            parsed_items,
                            (&slots, &blocks),
                            entering.clone(),
                            &sites,
                        )
                    },
                )
                .map_err(|failure| Failure {
                    error: failure.error.unimported(
                        |name, wanted| self.own_candidates(&context, name, wanted),
                        placement,
                    ),
                    ..failure
                })?;
            around.extend(resolved.around);
            #[cfg(all(feature = "rust-import", not(target_arch = "wasm32")))]
            if resolved.root.is_some() {
                root_types = resolved.root;
            }
            aliases.extend(resolved.aliases);
            // A reused shell is numbered as the program numbers its items now.
            for (shell, slot) in resolved.shells.into_iter().zip(slots.iter().flatten()) {
                shells.push(item::Shell {
                    id: slot.id,
                    ..shell
                });
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

        #[cfg(all(feature = "rust-import", not(target_arch = "wasm32")))]
        let natives = self.resolve_natives(root_types.as_ref())?;
        let definitions = finish_definitions(
            std::mem::take(&mut self.type_declarations),
            &shells,
            self.host,
            &self.expanded,
        )
        .map_err(Failure::from_located)?;
        let unimported = |error: LocatedError| {
            let failure = Failure::from_located(error);
            let placement =
                placements.get(&program::source::Id::from_index(failure.primary.index()));
            Failure {
                error: failure.error.unimported(
                    |name, wanted| {
                        contexts
                            .get(&failure.primary)
                            .map_or_else(Vec::new, |context| {
                                self.own_candidates(context, name, wanted)
                            })
                    },
                    placement.copied(),
                ),
                ..failure
            }
        };
        let (methods, builtin_methods, std_traits) =
            super::resolve_methods(&mut shells, &definitions, self.host).map_err(unimported)?;
        super::resolve_functions(&mut shells).map_err(unimported)?;
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
            .tables
            .packages()
            .iter()
            .find(|package| package.name() == self.package.as_str());
        let entry_crate = entry
            .and_then(|package| package.entry().and(package.rust_crate()))
            .map(ImmutableString::from);
        let features = entry
            .map(|package| package.features().clone())
            .unwrap_or_default();
        let workspace = entry.and_then(|package| package.workspace().cloned());
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
            qualified_aliases: qualified_calls.aliases,
            qualified_pattern_owners,
            qualified_rust: qualified_calls.rust,
            qualified_traits: qualified_calls.traits,
            trait_names,
            entry_crate,
            #[cfg(feature = "rust-import")]
            mirrors: self.mirrors,
            #[cfg(feature = "rust-import")]
            own: self.own,
            crates: prelude,
            features,
            workspace,
            aliases,
            scripts,
            instances: super::Instances {
                #[cfg(all(feature = "rust-import", not(target_arch = "wasm32")))]
                natives,
                ..super::Instances::default()
            },
            lexical,
            placements,
            macros,
        };
        super::validate_invocation_storage(&program).map_err(Failure::from_located)?;
        super::infinite::Layouts::of(&program)
            .reject()
            .map_err(Failure::from_located)?;
        Ok(program)
    }

    /// `parsed_items`, the items of the unit `context`, with the slots and
    /// block items it declared, resolved: their shells, the type aliases
    /// the unit names, and the names the blocks around each derive of its
    /// items bind, by the derive's expansion in `sites`. `entering` holds
    /// the names the blocks around the derive the unit expands bind.
    fn resolve_unit(
        &self,
        context: &Context,
        parsed_items: Box<[crate::item::Parsed]>,
        (slots, blocks): (&[Option<Slot>], &BlockItems),
        entering: BTreeMap<ImmutableString, type_ref::Target>,
        sites: &[(source::Id, Span)],
    ) -> Result<kept::Resolved, Failure> {
        let (mut uses, mut types, _occupied) =
            self.resolve_unit_bindings(context, &parsed_items)?;
        // An expansion's blocks see what its item's blocks bind, as its
        // module does: serde's `__Visitor` of a body's `struct Entry`,
        // declared in the expansion's `const _` block, names `Entry`
        // (#1938).
        for (name, target) in &entering {
            if self.tables.declaration(&context.address(name)).is_none() {
                types.insert(name.clone(), target.clone());
            }
        }
        let block_types =
            self.resolve_block_bindings(context, &parsed_items, blocks, &types, &mut uses)?;
        let mut around = Vec::new();
        for (expansion, derive) in sites {
            let Some(block) = super::block::innermost(&block_types, *derive) else {
                continue;
            };
            let names = block
                .iter()
                .filter(|(name, target)| types.get(*name) != Some(*target))
                .map(|(name, target)| (name.clone(), target.clone()))
                .collect();
            around.push((*expansion, names));
        }
        let root = (context.source == self.entry_source).then(|| types.clone());
        // The host modules this unit's `use`s bind, by name: an imported
        // trait is one (`use freya::prelude::IntoElement;`). A crate
        // trait it imports is known by its name, never a host path.
        let modules = uses
            .iter()
            .flatten()
            .flat_map(|used| used.bindings.iter())
            .filter(|binding| {
                matches!(
                    binding.target,
                    item::Import::Module | item::Import::ModuleItem { .. }
                ) && !matches!(
                    self.resolve_path(
                        context,
                        &["self".into(), binding.name.text.clone()],
                        binding.name.span,
                        &mut BTreeSet::new(),
                    ),
                    Ok(Target::Trait(_))
                )
            })
            .map(|binding| (binding.name.text.clone(), std_module(&binding.qualified)))
            .collect::<BTreeMap<_, _>>();
        let aliases = super::source_aliases(
            program::source::Id::from_index(context.source.index()),
            &types,
        );
        let mut shells = Vec::new();
        for (index, parsed) in parsed_items.into_vec().into_iter().enumerate() {
            let Some(slot) = &slots[index] else {
                continue;
            };
            let types = parsed
                .scope()
                .and_then(|block| super::block::innermost(&block_types, block))
                .unwrap_or(&types);
            // A block's items see the traits the blocks around them declare
            // or import, which the module's scope doesn't hold.
            let scope = parsed.scope();
            let block_binds = |name: &ImmutableString| {
                scope.is_some_and(|here| {
                    blocks.iter().any(|(block, declared)| {
                        super::block::encloses(*block, here) && declared.contains_key(name)
                    }) || uses.iter().flatten().any(|used| {
                        used.header.scope.is_some()
                            && sees(used.header.scope, scope)
                            && (used.traits.contains(name)
                                || used
                                    .bindings
                                    .iter()
                                    .any(|binding| binding.name.text == *name))
                    })
                })
            };
            let parsed = {
                let mut parsed = parsed;
                qualify_opaque_bounds(&mut parsed, &modules);
                write_renamed_modules(&mut parsed, &modules);
                self.write_native_trait_objects(context, &mut parsed, &modules);
                self.write_outside_paths(context, &mut parsed);
                self.write_trait_names(context, &mut parsed, &block_binds)
                    .map_err(|error| Failure {
                        primary: context.source,
                        secondary: None,
                        error,
                    })?;
                parsed
            };
            let shell = super::resolve_item(
                slot.id,
                program::source::Id::from_index(context.source.index()),
                slot.key.clone(),
                uses[index].clone(),
                parsed,
                types,
                self.host,
                #[cfg(feature = "rust-import")]
                self.native.as_deref().map(|capture| {
                    capture
                        .at(program::source::Id::from_index(context.source.index()))
                        .with_names(&_occupied)
                }),
            )
            .map_err(|error| {
                let restated = self.unresolved_type(context, &modules, error.clone());
                #[cfg(feature = "rust-import")]
                if let Some(capture) = self.native.as_deref() {
                    capture.restate(
                        program::source::Id::from_index(context.source.index()),
                        &error,
                        &restated,
                    );
                }
                Failure {
                    primary: context.source,
                    secondary: None,
                    error: restated,
                }
            })?;
            shells.push(shell);
        }
        Ok(kept::Resolved {
            entering,
            sites: sites.to_vec(),
            shells,
            aliases,
            around,
            root,
        })
    }

    /// Each recorded instance, by the path native code's call names its
    /// function or method by, its types read in the crate root's names
    /// (D869); the template that path names is found where instances are
    /// compiled. One whose function is gone, or whose types rawr can't
    /// write, has no instance: its function was changed since the record,
    /// so native code is built again and records it anew.
    #[cfg(all(feature = "rust-import", not(target_arch = "wasm32")))]
    fn resolve_natives(
        &self,
        root: Option<&BTreeMap<ImmutableString, type_ref::Target>>,
    ) -> Result<Vec<super::record::Native>, Failure> {
        let Some(root) = root else {
            return Ok(Vec::new());
        };
        let source = program::source::Id::from_index(self.entry_source.index());
        let Some(krate) = self
            .tables
            .packages()
            .iter()
            .find(|package| package.name() == self.package.as_str())
            .and_then(|package| package.rust_crate())
        else {
            return Ok(Vec::new());
        };
        let mut natives = Vec::new();
        for instance in &self.natives {
            let mut types = Vec::new();
            for written in &instance.types {
                let Some(parsed) = super::record::written(written, krate) else {
                    break;
                };
                let ty = type_ref::resolve(
                    parsed,
                    root,
                    self.host,
                    self.native.as_deref().map(|capture| capture.at(source)),
                )
                .map_err(|error| Failure {
                    primary: self.entry_source,
                    secondary: None,
                    error,
                })?;
                types.push(ty);
            }
            if types.len() == instance.types.len() {
                natives.push(super::record::Native {
                    path: instance.path.as_str().into(),
                    types: types.into_boxed_slice(),
                    key: crate::package::instances::key(&instance.name).into(),
                });
            }
        }
        Ok(natives)
    }

    /// Resolve each type alias's target in the module that declares it.
    /// An alias may name one another module declares, so the modules
    /// resolve theirs until every alias has its target or none more does.
    fn resolve_aliases(&mut self) -> Result<(), Failure> {
        let units = std::mem::take(&mut self.units);
        let resolved = self.resolve_aliases_of(&units);
        self.units = units;
        resolved
    }

    /// [`Self::resolve_aliases`] of `units`, each one's items read where
    /// it holds them (#1850).
    fn resolve_aliases_of(&mut self, units: &[Unit]) -> Result<(), Failure> {
        let declares = |unit: &Unit| {
            unit.parsed
                .items()
                .iter()
                .any(|parsed| matches!(parsed, crate::item::Parsed::TypeAlias(_)))
        };
        if !units.iter().any(declares) {
            return Ok(());
        }
        loop {
            let mut resolved = false;
            let mut first = None;
            for unit in units.iter().filter(|unit| declares(unit)) {
                let context = &unit.context;
                let items = unit.parsed.items();
                let (_, mut types, _) = self.resolve_unit_bindings(context, items)?;
                let before = items
                    .iter()
                    .filter(|parsed| {
                        matches!(parsed, crate::item::Parsed::TypeAlias(alias)
                            if matches!(types.get(&alias.name.text),
                                Some(type_ref::Target::Alias(type_ref::Alias { target: None, .. }))))
                    })
                    .count();
                let error = super::resolve_aliases(items, &mut types, self.host);
                for parsed in items {
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
                        .tables
                        .declaration_mut(&context.address(&alias.name.text))
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

    /// Whether each restricted visibility's path names an ancestor of the
    /// module its item is declared in, as rustc requires of `pub(in ..)`.
    fn validate_restrictions(&self) -> Result<(), Failure> {
        for unit in &self.units {
            for parsed in unit.parsed.items() {
                for visibility in visibilities(parsed) {
                    self.validate_restriction(&unit.context, visibility)
                        .map_err(|error| Failure {
                            primary: unit.context.source,
                            secondary: None,
                            error,
                        })?;
                }
            }
        }
        Ok(())
    }

    /// rustc's E0433 for a restriction's path through a module that doesn't
    /// exist or above the crate root, and its E0742 for one to a module that
    /// is no ancestor.
    fn validate_restriction(
        &self,
        context: &Context,
        visibility: &crate::item::Visibility,
    ) -> Result<(), Error> {
        let module = &context.namespace.modules;
        let (Some(restricted), Some(path)) = (
            visibility.restricted(module),
            visibility.restriction.as_deref(),
        ) else {
            return Ok(());
        };
        let scope = match restricted {
            crate::item::Restricted::AboveRoot(segment) => {
                let within = std::iter::once("crate")
                    .chain(module.iter().map(ImmutableString::as_str))
                    .collect::<Vec<_>>()
                    .join("::");
                return Err(Error::StdPath {
                    code: "E0433",
                    message: format!("too many leading `super` keywords within `{within}`").into(),
                    label: Some("this `super` would go above the crate root".into()),
                    note: None,
                    reference: segment.span,
                });
            }
            crate::item::Restricted::Module(scope) => scope,
        };
        if module.starts_with(&scope) {
            return Ok(());
        }
        // The first segment that names no module, from where the path
        // starts.
        let mut at = match path[0].text.as_str() {
            "crate" => Vec::new(),
            _ => module.to_vec(),
        };
        for (index, segment) in path.iter().enumerate() {
            match segment.text.as_str() {
                "crate" | "self" if index == 0 => continue,
                "super" => {
                    at.pop();
                    continue;
                }
                _ => {}
            }
            let address = Address {
                package: context.namespace.package.clone(),
                modules: at.clone().into_boxed_slice(),
                name: segment.text.clone(),
            };
            let declared = self.tables.declaration(&address);
            if !matches!(
                declared.map(|declared| &declared.target),
                Some(Target::Module)
            ) {
                let parent = path[index - 1].text.as_str();
                let label = match declared {
                    Some(declaration) => format!(
                        "expected type, found {} `{}` in `{parent}`",
                        self.private_kind(&declaration.target).noun(),
                        segment.text
                    ),
                    None if parent == "crate" => {
                        format!("could not find `{}` in the crate root", segment.text)
                    }
                    None => format!("could not find `{}` in `{parent}`", segment.text),
                };
                return Err(Error::StdPath {
                    code: "E0433",
                    message: format!("cannot find `{}` in `{parent}`", segment.text).into(),
                    label: Some(label.into()),
                    note: None,
                    reference: segment.span,
                });
            }
            at.push(segment.text.clone());
        }
        let last = path.last().expect("a restriction's path has a segment");
        Err(Error::StdPath {
            code: "E0742",
            message: "visibilities can only be restricted to ancestor modules".into(),
            label: None,
            note: None,
            reference: Span::new(path[0].span.start(), last.span.end()),
        })
    }

    fn validate_exports(&self) -> Result<(), Failure> {
        for (address, export) in self.tables.exports() {
            self.validated(address, export, || self.validate_export(address, export))?;
        }
        Ok(())
    }

    /// Whether the re-export `export` at `address` names an item or a
    /// variant, as `pub use` may.
    fn validate_export(&self, address: &Address, export: &Export) -> Result<(), Failure> {
        // `pub use self::Kind::Secondary;` re-exports a variant.
        let (context, path) = self.origin(&export.context, &export.path);
        if self.variant(&context, &path).is_some() {
            return Ok(());
        }
        let mut active = BTreeSet::new();
        active.insert(address.clone());
        self.resolve_any_path(&export.context, &export.path, export.span, &mut active)?;
        // A re-export reaches no further than what it re-exports: rustc's
        // E0364.
        if let Some((address, declaration)) = self.declared_at(&context, &path)
            && !export.reach.within(&declaration.reach)
        {
            let name = &address.name;
            let message = match &declaration.reach {
                super::Reach::Within(module) if module.modules.is_empty() => {
                    format!("`{name}` is only public within the crate, and cannot be re-exported outside")
                }
                _ => format!("`{name}` is private, and cannot be re-exported"),
            };
            return Err(Failure {
                primary: export.source,
                secondary: None,
                error: Error::StdPath {
                    code: "E0364",
                    message: message.into(),
                    label: None,
                    note: Some(
                        format!("consider marking `{name}` as `pub` in the imported module").into(),
                    ),
                    reference: export.written,
                },
            });
        }
        Ok(())
    }

    /// The crate declaration `path` names from `origin`, where no import
    /// is on the way (`Resolver::origin`), with its address.
    fn declared_at(
        &self,
        origin: &Context,
        path: &[ImmutableString],
    ) -> Option<(Address, &Declaration)> {
        let (name, prefix) = path.split_last()?;
        let modules = match prefix {
            [] => origin.namespace.modules.to_vec(),
            _ => self.local_glob_module(origin, prefix)?,
        };
        let address = Address {
            package: origin.namespace.package.clone(),
            modules: modules.into_boxed_slice(),
            name: name.clone(),
        };
        let declaration = self.tables.declaration(&address)?;
        Some((address, declaration))
    }

    /// The names a glob import (`use dep::module::*;`) binds: every name
    /// of that module this unit sees that no declaration or explicit import
    /// of this unit already binds.
    fn glob_paths(
        &self,
        context: &Context,
        parsed: &crate::item::Use,
        bound: &Bound,
    ) -> Result<Vec<UsePath>, Failure> {
        let crate::item::UseTree::Glob { prefix, span } = &parsed.tree else {
            return Ok(Vec::new());
        };
        let segments = path_segments(prefix);
        let segments = parsed
            .header
            .scope
            .and_then(|block| self.in_blocks(context, &segments, block))
            .unwrap_or(segments);
        let module: ImmutableString = segments.join("::").into();
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
                // A std module std doesn't have is marked at the segment
                // that misses, as rustc marks a group's prefix.
                _ => match super::std_path::import(&module, prefix.span) {
                    Err(Error::UnknownImport {
                        path,
                        missing: Unresolved::Item { module, .. } | Unresolved::Module(module),
                        ..
                    }) => Error::UnknownImport {
                        reference: prefix
                            .segments
                            .get(path.split("::").count() - 1)
                            .map_or(*span, |segment| segment.name.span),
                        path,
                        missing: Unresolved::Module(module),
                    },
                    Err(error) => error,
                    Ok(_) => Error::UnknownImport {
                        path: module,
                        reference: *span,
                        missing: Unresolved::Path,
                    },
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
    fn names_of_glob(
        &self,
        context: &Context,
        segments: &[ImmutableString],
    ) -> Option<Vec<ImmutableString>> {
        let module: ImmutableString = segments.join("::").into();
        if let Some((definition, _)) = self.script_enum(context, segments) {
            return self
                .tables
                .variants(&definition)
                .map(<[ImmutableString]>::to_vec);
        }
        if super::std_ordering(&self.through_use(context, segments)) {
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
        let package = &context.namespace.package;
        let declared = self
            .tables
            .declared_in(package, modules)
            .filter(|(_, declaration)| declaration.reach.reaches(&context.namespace))
            .map(|(address, _)| address);
        let exported = self
            .tables
            .exported_in(package, modules)
            .map(|(address, _)| address);
        let imported = self
            .tables
            .aliased_in(package, modules)
            .chain(self.tables.globbed_in(package, modules))
            .filter(|_| inside)
            .map(|(address, _)| address);
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
        // Each step follows a distinct import, so a cycle ends the walk
        // after as many steps as there are imports.
        let mut steps = 0..=self.tables.imports();
        loop {
            if steps.next().is_none() {
                self.tables.walked_imports();
                break;
            }
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
        if self.tables.declaration(&address).is_some() {
            return None;
        }
        self.tables
            .export(&address)
            .or_else(|| self.tables.alias(&address))
            .or_else(|| self.tables.glob(&address))
    }

    /// The variant `path` names from `context`, after [`Self::origin`]:
    /// `Shape::Circle` of an enum of this crate, which `super::Circle`
    /// reaches through the parent's import, std's `Ordering::Less`, or a
    /// crate enum's, as `iced::Alignment::Center` or the re-exported
    /// `iced::Center`.
    fn variant(&self, context: &Context, path: &[ImmutableString]) -> Option<item::Import> {
        let (last, prefix) = path.split_last()?;
        let Some((definition, identity)) = self.script_enum(context, prefix) else {
            if super::std_ordering(&self.through_use(context, prefix))
                && ORDERING_VARIANTS.contains(&last.as_str())
            {
                return Some(item::Import::OrderingVariant);
            }
            return self
                .host
                .enum_variant(&super::std_reexport(&path.join("::"), self.host))
                .map(|owner| item::Import::HostVariant { owner });
        };
        self.tables
            .variants(&definition)
            .is_some_and(|variants| variants.contains(last))
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
            [name] => match self.tables.declaration(&context.address(name)) {
                Some(declaration) => declaration.target.clone(),
                None => {
                    let address = context.address(name);
                    let alias = self
                        .through_alias(&address)
                        .or_else(|| self.tables.glob(&address))?;
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
            }) if self.tables.variants(&definition).is_some() => Some((definition, identity)),
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
        let mut bound = Bound::new();
        let declared = self
            .tables
            .declared_in(&context.namespace.package, &context.namespace.modules);
        for (address, declaration) in declared {
            let namespace = match declaration.target {
                Target::Module | Target::Trait(_) => item::Namespace::Type,
                Target::Item(_) => item::Namespace::Value,
                _ => item::Namespace::Both,
            };
            for side in namespace.sides() {
                bound.insert(
                    (address.name.clone(), *side),
                    (declaration.source, declaration.span, None),
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

        self.resolve_qualified_header_types(
            context,
            parsed_items,
            HeaderPaths::Module,
            &mut types,
        )?;
        let mut uses = vec![None; parsed_items.len()];

        let members = (0..parsed_items.len())
            .filter(|index| parsed_items[*index].scope().is_none())
            .collect::<Vec<_>>();
        self.bind_uses(
            context,
            parsed_items,
            &members,
            None,
            &mut types,
            &mut bound,
            &mut occupied,
            &mut uses,
        )?;
        super::module_header_types(parsed_items, &uses, self.host, &mut types);
        for (name, declaration) in modules {
            occupied.entry(name).or_insert(declaration);
        }
        Ok((uses, types, occupied))
    }

    /// Each derive's expansion with the source of its item and the span of
    /// the derive there.
    fn derive_sites(&self) -> BTreeMap<source::Id, (source::Id, Span)> {
        self.evidence
            .iter()
            .enumerate()
            .filter_map(|(index, evidence)| {
                let origin = evidence.origin()?;
                let item = self.evidence.iter().position(|candidate| {
                    candidate.origin().is_none() && candidate.identity() == origin.source.identity()
                })?;
                Some((
                    source::Id::from_index(index),
                    (source::Id::from_index(item), origin.span),
                ))
            })
            .collect()
    }

    /// Bind the names the `use`s among `members`, by their indices among
    /// `parsed_items`, import into the scope `scope` (`None` for the
    /// module), whose names so far are `types`, `bound` and `occupied`.
    #[allow(clippy::too_many_arguments)]
    fn bind_uses(
        &self,
        context: &Context,
        parsed_items: &[crate::item::Parsed],
        members: &[usize],
        scope: Option<Span>,
        types: &mut BTreeMap<ImmutableString, type_ref::Target>,
        bound: &mut Bound,
        occupied: &mut BTreeMap<ImmutableString, (source::Id, Span)>,
        uses: &mut [Option<item::Use>],
    ) -> Result<(), Failure> {
        // Explicit imports first: a name they or a declaration bind shadows
        // the same name from a glob import, as in Rust.
        let glob = |parsed: &crate::item::Parsed| {
            matches!(parsed, crate::item::Parsed::Use(used)
                if matches!(used.tree, crate::item::UseTree::Glob { .. }))
        };
        let mut order = members.to_vec();
        order.sort_by_key(|index| glob(&parsed_items[*index]));
        // The members of the first `use` that name nothing, by its span: a
        // group's hoisted paths share it. rustc reports them in one error.
        let mut unresolved: Option<(Span, Failure)> = None;
        for index in order {
            let crate::item::Parsed::Use(parsed) = &parsed_items[index] else {
                continue;
            };
            if unresolved
                .as_ref()
                .is_some_and(|(span, ..)| *span != parsed.span)
            {
                continue;
            }
            let mut bindings = Vec::new();
            let mut traits = Vec::new();

            let globbed = self.glob_paths(context, parsed, bound)?;
            for (name, path) in use_paths(parsed).into_iter().chain(globbed) {
                let path = scope
                    .and_then(|block| self.in_blocks(context, &path, block))
                    .unwrap_or(path);
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
                        .filter(|used| sees(used.header.scope, scope))
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
                // A dependency's macro the facades hold no item at, as `use
                // rust_decimal_macros::dec;` names a proc macro, is bound in
                // the macro namespace for rustc to expand its calls. Against
                // native code's retained facades, a path they miss is a miss
                // first: preparation demands it, and binds it as a macro
                // only when the crate holds no item there. One rustc
                // resolved as a macro before stays one.
                let variant = variant.or_else(|| {
                    let qualified = super::std_reexport(&spelled.join("::"), self.host);
                    if self.host.entry(&qualified).is_some() {
                        None
                    } else if self.host.derive_macro(&qualified) {
                        Some(item::Import::Derive)
                    } else if self.missed_macro_use(context, &qualified) {
                        None
                    } else {
                        self.tables
                            .prelude()
                            .is_some_and(|prelude| prelude.crate_macro(self.host, &qualified))
                            .then_some(item::Import::CrateMacro)
                    }
                });
                let target = match variant {
                    Some(_) => Target::Module,
                    None => {
                        let mut active = BTreeSet::new();
                        match self.resolve_any_path(context, &path, name.span, &mut active) {
                            Ok(target) => target,
                            Err(failure) => {
                                #[cfg(feature = "rust-import")]
                                let missed = failure.error.clone();
                                let failure = self.unresolved_use(
                                    context,
                                    &parsed.tree,
                                    &path,
                                    name.span,
                                    failure,
                                );
                                let error = super::whole_import(failure.error, &parsed.tree);
                                // A miss against the retained facades stays
                                // the miss of the import rustc's E0432 marks.
                                #[cfg(feature = "rust-import")]
                                if let Some(capture) = self.native.as_deref() {
                                    capture.restate(
                                        program::source::Id::from_index(context.source.index()),
                                        &missed,
                                        &error,
                                    );
                                }
                                if !matches!(error, Error::UnknownImport { .. }) {
                                    return Err(Failure { error, ..failure });
                                }
                                unresolved = Some(match unresolved.take() {
                                    Some((span, first)) => (
                                        span,
                                        Failure {
                                            error: first.error.with_unknown_import(error),
                                            ..first
                                        },
                                    ),
                                    None => (parsed.span, Failure { error, ..failure }),
                                });
                                continue;
                            }
                        }
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
                if let Target::Trait(address) = &target {
                    traits.push(self.known_trait_name(address.clone()));
                }
                let import = match target {
                    _ if let Some(variant) = variant.clone() => variant,
                    Target::Module | Target::Trait(_) => item::Import::Module,
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
                        | type_ref::Target::Opaque(_)
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
                let written = |side| super::Import {
                    written: parsed.written(&name),
                    rename: parsed.rename(&name),
                    kind: import.kind(side, &qualified),
                };
                let previous = import.namespace().sides().iter().find_map(|side| {
                    let previous = bound.insert(
                        (name.text.clone(), *side),
                        (context.source, name.span, Some(written(*side))),
                    );
                    previous.map(|previous| (*side, previous))
                });
                if let Some((side, (previous_source, previous_span, imported))) = previous {
                    return Err(Failure {
                        primary: context.source,
                        secondary: Some(previous_source),
                        error: Error::duplicate(
                            name.text.clone(),
                            side.noun(),
                            written(side),
                            (previous_span, imported),
                            previous_source == context.source,
                        ),
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
                traits: traits.into_boxed_slice(),
                span: parsed.span,
            });
        }
        match unresolved {
            Some((_, failure)) => Err(failure),
            None => Ok(()),
        }
    }

    /// The names each block of the unit that binds a type sees: its own
    /// items and imports over those of the scopes it is nested in, the
    /// module's `types` outermost. The imports of its `use`s go in `uses`.
    fn resolve_block_bindings(
        &self,
        context: &Context,
        parsed_items: &[crate::item::Parsed],
        blocks: &BlockItems,
        types: &BTreeMap<ImmutableString, type_ref::Target>,
        uses: &mut [Option<item::Use>],
    ) -> Result<BTreeMap<Span, BTreeMap<ImmutableString, type_ref::Target>>, Failure> {
        let mut block_types = BTreeMap::new();
        for block in super::block::scopes(parsed_items) {
            let members = (0..parsed_items.len())
                .filter(|index| parsed_items[*index].scope() == Some(block))
                .collect::<Vec<_>>();
            let declared = blocks.get(&block);
            // The types its items' headers name through a block's module.
            let mut through = BTreeMap::new();
            self.resolve_qualified_header_types(
                context,
                members.iter().map(|index| &parsed_items[*index]),
                HeaderPaths::Block,
                &mut through,
            )?;
            let binds_type = !through.is_empty()
                || declared.is_some_and(|items| {
                    items
                        .values()
                        .any(|(_, target)| matches!(target, Target::Type(_)))
                })
                || members
                    .iter()
                    .any(|index| matches!(parsed_items[*index], crate::item::Parsed::Use(_)));
            if !binds_type {
                continue;
            }
            let mut scoped = super::block::innermost(&block_types, block)
                .unwrap_or(types)
                .clone();
            scoped.extend(through);
            let mut bound = Bound::new();
            let mut occupied = BTreeMap::new();
            for (name, (span, target)) in declared.into_iter().flatten() {
                let namespace = match target {
                    Target::Module | Target::Trait(_) => item::Namespace::Type,
                    Target::Item(_) => item::Namespace::Value,
                    _ => item::Namespace::Both,
                };
                for side in namespace.sides() {
                    bound.insert((name.clone(), *side), (context.source, *span, None));
                }
                occupied.insert(name.clone(), (context.source, *span));
                if let Target::Type(target) = target {
                    scoped.insert(name.clone(), target.clone());
                }
            }
            self.bind_uses(
                context,
                parsed_items,
                &members,
                Some(block),
                &mut scoped,
                &mut bound,
                &mut occupied,
                uses,
            )?;
            let members = members
                .iter()
                .map(|index| &parsed_items[*index])
                .collect::<Vec<_>>();
            let visible = uses
                .iter()
                .filter(|used| {
                    used.as_ref()
                        .is_some_and(|used| sees(used.header.scope, Some(block)))
                })
                .cloned()
                .collect::<Vec<_>>();
            super::module_header_types(&members, &visible, self.host, &mut scoped);
            if let Some(error) = super::resolve_aliases(&members, &mut scoped, self.host) {
                return Err(Failure {
                    primary: context.source,
                    secondary: None,
                    error,
                });
            }
            block_types.insert(block, scoped);
        }
        Ok(block_types)
    }

    /// The types the `paths` among the qualified paths of `parsed_items`'
    /// headers name, into `types` by how they are written.
    fn resolve_qualified_header_types<'p>(
        &self,
        context: &Context,
        parsed_items: impl IntoIterator<Item = &'p crate::item::Parsed>,
        paths: HeaderPaths,
        types: &mut BTreeMap<ImmutableString, type_ref::Target>,
    ) -> Result<(), Failure> {
        let mut written_paths = Vec::new();
        // Where each impl's trait is written, which names a trait.
        let mut traits = BTreeSet::new();
        for item in parsed_items {
            if let crate::item::Parsed::Impl(item) = item
                && let Some(path) = &item.trait_
            {
                traits.insert(path.span);
            }
            collect_item_type_paths(item, &mut written_paths);
        }
        for path in written_paths {
            let written = path_segments(path);
            let scoped = self.in_blocks(context, &written, path.span);
            let through = if scoped.is_some() {
                HeaderPaths::Block
            } else {
                HeaderPaths::Module
            };
            if through != paths {
                continue;
            }
            let segments = scoped.as_deref().unwrap_or(&written);
            if path.global {
                self.reject_local_global(context, segments, path.span)?;
                continue;
            }
            if segments.len() < 2 || !self.is_script_path(context, segments) {
                continue;
            }
            let mut active = BTreeSet::new();
            let target = self
                .resolve_path(context, segments, path.span, &mut active)
                .map_err(|failure| {
                    let noun = if traits.contains(&path.span) {
                        "trait"
                    } else {
                        "type"
                    };
                    let failure = self.unresolved_type_path(context, path, noun, failure);
                    // A segment's span where the path resolves as written.
                    let segment_span = |index: usize| {
                        (*segments == *written)
                            .then(|| path.segments.get(index).map(|segment| segment.name.span))
                            .flatten()
                    };
                    self.missing_segment(context, segments, segment_span, failure)
                })?;
            // An impl's trait path that names another item: rustc's E0404. A
            // trait of std or of a Rust crate, native code's included, is a
            // module of its host (`use rand::Rng;`), so a module is another
            // item only when the program declares it.
            let names_trait = match target {
                Target::Trait(_) => true,
                Target::Module | Target::HostModuleItem { .. } => {
                    !self.names_script_module(context, segments, &mut BTreeSet::new())
                }
                Target::Type(_) | Target::Item(_) | Target::HostItem { .. } => false,
            };
            if traits.contains(&path.span) && !names_trait {
                return Err(Failure {
                    primary: context.source,
                    secondary: None,
                    error: Error::StdPath {
                        code: "E0404",
                        message: format!(
                            "expected trait, found {} `{}`",
                            self.private_kind(&target).noun(),
                            written.join("::")
                        )
                        .into(),
                        label: Some("not a trait".into()),
                        note: None,
                        reference: path.span,
                    },
                });
            }
            match target {
                Target::Type(target) => {
                    types.insert(written.join("::").into(), target);
                }
                Target::Module
                | Target::Trait(_)
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

    /// A `use` or glob binding of a script module in `context` (`use
    /// super::config;`, or `use super::*;` reaching the parent's), which a
    /// longer path may start with.
    fn module_alias(&self, context: &Context, name: &str) -> Option<&Export> {
        let address = context.address(name);
        let alias = self
            .through_alias(&address)
            .or_else(|| self.tables.export(&address))
            .or_else(|| self.tables.glob(&address))?;
        let module = self.is_direct_script_path(&alias.context, &alias.path)
            && self.names_script_module(&alias.context, &alias.path, &mut BTreeSet::new());
        module.then_some(alias)
    }

    /// Whether `path`, written in `context`, names a module of this crate,
    /// each segment through the declarations and the imports of the module
    /// before it: a std or a library module a `use` there binds is none.
    fn names_script_module(
        &self,
        context: &Context,
        path: &[ImmutableString],
        active: &mut BTreeSet<Address>,
    ) -> bool {
        let path = through_self(path);
        let supers = path
            .iter()
            .take_while(|segment| *segment == "super")
            .count();
        let modules = context.namespace.modules.as_ref();
        let (base, tail) = match path.first().map(ImmutableString::as_str) {
            Some("super") => match modules.len().checked_sub(supers) {
                Some(parent) => (&modules[..parent], &path[supers..]),
                None => return false,
            },
            Some("crate") => (&[][..], &path[1..]),
            Some("self") => (modules, &path[1..]),
            _ => (modules, path),
        };
        let mut at = base.to_vec();
        for (index, name) in tail.iter().enumerate() {
            let address = Address {
                package: context.namespace.package.clone(),
                modules: at.as_slice().into(),
                name: name.clone(),
            };
            if let Some(declaration) = self.tables.declaration(&address) {
                if !matches!(declaration.target, Target::Module) {
                    return false;
                }
                at.push(name.clone());
                continue;
            }
            let Some(import) = self
                .through_alias(&address)
                .or_else(|| self.tables.export(&address))
                .or_else(|| self.tables.glob(&address))
            else {
                return false;
            };
            if !active.insert(address) {
                return false;
            }
            let expanded = import
                .path
                .iter()
                .chain(&tail[index + 1..])
                .cloned()
                .collect::<Vec<_>>();
            return self.names_script_module(&import.context, &expanded, active);
        }
        true
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
            .or_else(|| self.tables.export(&address))?;
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
    /// The path `segments` spells through the `use` that binds its first
    /// segment in `context`, as rustc resolves a path: `Ordering` after
    /// `use std::cmp::Ordering;` is `std::cmp::Ordering`.
    fn through_use(&self, context: &Context, segments: &[ImmutableString]) -> String {
        let alias = segments
            .first()
            .and_then(|first| self.through_alias(&context.address(first)))
            .filter(|alias| alias.path.first() != segments.first());
        match alias {
            Some(alias) => alias
                .path
                .iter()
                .chain(&segments[1..])
                .map(ImmutableString::as_str)
                .collect::<Vec<_>>()
                .join("::"),
            None => segments.join("::"),
        }
    }

    fn through_alias(&self, address: &Address) -> Option<&Export> {
        let first = self.tables.alias(address)?;
        let Some(other) = self.tables.other_alias(address) else {
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

    /// `path` as `context`'s module resolves it when it is written at
    /// `at`, when a block around `at` declares a module of its first
    /// segment: the path through that module's segment
    /// (`super::block::module_segment`), the innermost block's first. A
    /// block's module is no module's child, so nowhere else does its name
    /// reach it.
    fn in_blocks(
        &self,
        context: &Context,
        path: &[ImmutableString],
        at: Span,
    ) -> Option<Box<[ImmutableString]>> {
        let (first, rest) = path.split_first()?;
        let blocks = self.block_modules.get(&context.source)?;
        let expansions = self
            .expansions
            .get(&context.source)
            .map_or(&[][..], AsRef::as_ref);
        let segment = super::macros::sites(expansions, at)
            .find_map(|at| {
                blocks
                    .range(..=at)
                    .rev()
                    .filter(|(block, _)| super::block::encloses(**block, at))
                    .find_map(|(_, modules)| modules.get(first))
            })
            .map(|(_, segment)| segment)?;
        Some(std::iter::once(segment).chain(rest).cloned().collect())
    }

    fn is_child_module(&self, context: &Context, name: &str) -> bool {
        self.tables
            .declaration(&context.address(name))
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
        let alias = self.tables.rust_crate(package)?;
        let mut item = modules.to_vec();
        item.push(name.clone());
        if !self.tables.is_native_path(package, &item) {
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
        let path = |path: &mut Path, _: Place| {
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

    /// Each path of `item`'s signatures and bounds through this crate's
    /// modules that lands on a std or crate item through an import
    /// ([`Self::rust_spelling`]), written as that item's own path, as
    /// rustc resolves it: `super::fmt::Formatter` after the parent's `use
    /// std::fmt;` is `std::fmt::Formatter`. The written path's last
    /// segment keeps its arguments and its span.
    fn write_outside_paths(&self, context: &Context, item: &mut crate::item::Parsed) {
        use crate::item::type_ref::{Path, Segment, DYN_SEGMENT};
        let app = self.tables.rust_crate(context.package);
        let path = |path: &mut Path, _: Place| {
            let [first, .., last] = &path.segments[..] else {
                return;
            };
            if path.global
                || first.name.text.as_str() == DYN_SEGMENT
                || path.segments[..path.segments.len() - 1]
                    .iter()
                    .any(|segment| !segment.arguments.is_empty())
            {
                return;
            }
            let written = path
                .segments
                .iter()
                .map(|segment| segment.name.text.clone())
                .collect::<Vec<_>>();
            if !self.is_script_path(context, &written) {
                return;
            }
            // A path into a `.rs` module keeps its types' own resolution.
            let Some(spelled) = self
                .rust_spelling(context, &written)
                .filter(|spelled| app.is_none_or(|app| spelled[0] != app))
            else {
                return;
            };
            let (at, span) = (first.name.span, first.span);
            let last = last.clone();
            let count = spelled.len();
            path.segments = spelled
                .into_iter()
                .enumerate()
                .map(|(index, text)| {
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
                })
                .collect();
        };
        each_signature_path(item, &path);
    }

    /// The name the lowering knows the crate trait at `address` by: the
    /// path of [`Resolver::trait_names`] when another module's trait has its
    /// name, else its own name.
    fn known_trait_name(&self, address: Address) -> ImmutableString {
        self.trait_names
            .get(&address)
            .cloned()
            .unwrap_or(address.name)
    }

    /// The name each unit writes a trait of [`Resolver::trait_names`] by, its
    /// own or one it imports, with the path the trait is known by, so a
    /// body's `Shape::size(&c)` calls the trait the module names.
    fn source_trait_names(
        &self,
    ) -> BTreeMap<(program::source::Id, ImmutableString), ImmutableString> {
        let names = self
            .trait_names
            .keys()
            .map(|address| &address.name)
            .collect::<BTreeSet<_>>();
        let mut known = BTreeMap::new();
        for unit in &self.units {
            for name in &names {
                let path = ["self".into(), (*name).clone()];
                let resolved =
                    self.resolve_path(&unit.context, &path, Span::NONE, &mut BTreeSet::new());
                if let Ok(Target::Trait(address)) = resolved
                    && let Some(path) = self.trait_names.get(&address)
                {
                    let source = program::source::Id::from_index(unit.context.source.index());
                    known.insert((source, (*name).clone()), path.clone());
                }
            }
        }
        known
    }

    /// Each path of `item`'s signatures and bounds that names a crate
    /// trait, through its module (`m::Shape`, `super::Shape`,
    /// `crate::home::IntoView`) or by a name a `use` gives it (`Shape as
    /// Outline`), written as the one segment the trait is known by
    /// ([`Resolver::known_trait_name`]), its own arguments kept, as the
    /// module's scope resolves it; and a trait declaration whose name
    /// another module's trait has renamed to its path: two traits of one
    /// name are two items, as rustc's are.
    ///
    /// A trait's place that writes a crate trait's name nothing in scope
    /// binds (not the module's items, its `use`s and globs, the blocks
    /// around the item, `block_binds`, or the prelude) is rustc's E0405:
    /// a crate trait is named through the scope of the module naming it,
    /// never by its name alone.
    fn write_trait_names(
        &self,
        context: &Context,
        item: &mut crate::item::Parsed,
        block_binds: &impl Fn(&ImmutableString) -> bool,
    ) -> Result<(), Error> {
        use crate::item::type_ref::{Path, Segment, DYN_SEGMENT};
        if self.trait_spellings.is_empty() {
            return Ok(());
        }
        let refused = RefCell::new(None);
        let path = |path: &mut Path, place: Place| {
            let skip = usize::from(
                path.segments
                    .first()
                    .is_some_and(|first| first.name.text.as_str() == DYN_SEGMENT),
            );
            let place = if skip == 0 { place } else { Place::Trait };
            let written = &path.segments[skip..];
            let Some(last) = written.last() else {
                return;
            };
            if !self.trait_spellings.contains(&last.name.text) {
                return;
            }
            let mut names = written
                .iter()
                .map(|segment| segment.name.text.clone())
                .collect::<Vec<_>>();
            if names.len() == 1 {
                names.insert(0, "self".into());
            }
            let resolved = self.resolve_path(context, &names, path.span, &mut BTreeSet::new());
            let address = match resolved {
                Ok(Target::Trait(address)) => address,
                Err(_)
                    if place == Place::Trait
                        && written.len() == 1
                        && !self.tables.binds(&context.address(&last.name.text))
                        && !block_binds(&last.name.text)
                        && !prelude_trait(&last.name.text) =>
                {
                    refused
                        .borrow_mut()
                        .get_or_insert_with(|| super::unknown_trait(&last.name));
                    return;
                }
                _ => return,
            };
            let known = self.known_trait_name(address);
            if let [only] = written
                && only.name.text == known
            {
                return;
            }
            let segment = Segment {
                name: crate::item::Name {
                    text: known,
                    span: last.name.span,
                },
                ..last.clone()
            };
            path.segments = path.segments[..skip]
                .iter()
                .cloned()
                .chain(std::iter::once(segment))
                .collect();
            path.global = false;
        };
        each_signature_path(item, &path);
        if let crate::item::Parsed::Trait(declared) = item {
            if let Some(known) = self.trait_names.get(&context.address(&declared.name.text)) {
                declared.name.text = known.clone();
            }
            for bound in declared
                .supertraits
                .iter_mut()
                .chain(declared.associated_bounds.iter_mut().flatten())
                .chain(
                    declared
                        .generics
                        .iter_mut()
                        .flat_map(|generic| generic.bounds.iter_mut()),
                )
            {
                path(bound, Place::Trait);
            }
        }
        refused.into_inner().map_or(Ok(()), Err)
    }

    /// `crate::…`, `super::…` or child-module paths that land in a `.rs`
    /// module of the current package, or name one, spelled through the
    /// app's crate name. A `use` that binds a `.rs` module (`use
    /// crate::units;`) so binds the app crate's module, and paths through
    /// the binding (`units::BONUS`) reach its items as rustc's do. One
    /// that goes through an import of a std or crate item, as
    /// `super::fmt::Display` after the parent's `use std::fmt;`, or
    /// `m::Display` after `m`'s `pub use std::fmt::Display;`, is that
    /// item's own path (`std::fmt::Display`).
    fn rust_spelling(
        &self,
        context: &Context,
        path: &[ImmutableString],
    ) -> Option<Vec<ImmutableString>> {
        self.spelling_from(context, path, &mut BTreeSet::new())
    }

    /// [`Self::rust_spelling`], through no import in `active` again: a
    /// cycle of imports names nothing.
    fn spelling_from(
        &self,
        context: &Context,
        path: &[ImmutableString],
        active: &mut BTreeSet<Address>,
    ) -> Option<Vec<ImmutableString>> {
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
        } else if path.len() > 1
            && let Some(alias) = self.module_alias(context, first)
        {
            // `use crate::theme;` then `theme::text::base`: the path goes
            // on from the module the binding names, in that module's own
            // namespace, as rustc resolves it.
            let expanded = alias
                .path
                .iter()
                .chain(&path[1..])
                .cloned()
                .collect::<Vec<_>>();
            return self.spelling_from(&alias.context, &expanded, active);
        } else {
            return None;
        }
        if let Some(alias) = self.tables.rust_crate(context.package)
            && self.tables.names_native(context.package, &absolute)
        {
            return Some(
                std::iter::once(ImmutableString::from(alias))
                    .chain(absolute)
                    .collect(),
            );
        }
        self.imported_spelling(context, &absolute, active)
    }

    /// The crate spelling of the absolute path `absolute` of `context`'s
    /// package when one of its segments is an import of the module before
    /// it rather than a declaration there: `super::icon` from a module
    /// whose parent's `use super::*;` binds `icon` is the path that glob
    /// names, spelled from the glob's own module, as rustc follows it.
    fn imported_spelling(
        &self,
        context: &Context,
        absolute: &[ImmutableString],
        active: &mut BTreeSet<Address>,
    ) -> Option<Vec<ImmutableString>> {
        for (at, name) in absolute.iter().enumerate() {
            let address = Address {
                package: context.namespace.package.clone(),
                modules: absolute[..at].into(),
                name: name.clone(),
            };
            if let Some(declaration) = self.tables.declaration(&address) {
                if matches!(declaration.target, Target::Module) {
                    continue;
                }
                return None;
            }
            // An import `context` doesn't see spells nothing: the path is
            // its privacy error, as rustc's E0603.
            let import = self
                .tables
                .alias(&address)
                .or_else(|| self.tables.export(&address))
                .or_else(|| self.tables.glob(&address))
                .filter(|import| import.reach.reaches(&context.namespace))?;
            if !active.insert(address) {
                return None;
            }
            let expanded = import
                .path
                .iter()
                .chain(&absolute[at + 1..])
                .cloned()
                .collect::<Vec<_>>();
            return self.import_spelling(&import.context, expanded, active);
        }
        None
    }

    /// The spelling of `path`, an import's path in its module `context`
    /// expanded by the segments after it: [`Self::spelling_from`] for a
    /// path of this crate, and for one outside it, the std or crate path
    /// its first segment names through each `use` that binds it
    /// (`fmt::Display` after `use std::fmt;` is `std::fmt::Display`), as
    /// rustc resolves it. `None` where that first segment is an item of
    /// this crate, as an enum is the prefix of `use Shape::*;`'s paths, or
    /// where the path names nothing outside it.
    fn import_spelling(
        &self,
        context: &Context,
        path: Vec<ImmutableString>,
        active: &mut BTreeSet<Address>,
    ) -> Option<Vec<ImmutableString>> {
        if self.is_script_path(context, &path) {
            return self.spelling_from(context, &path, active);
        }
        if let Some((address, alias)) = self.crate_alias(context, &path) {
            if !active.insert(address) {
                return None;
            }
            let expanded = alias.path.iter().chain(&path[1..]).cloned().collect();
            return self.import_spelling(&alias.context, expanded, active);
        }
        let first = path.first()?;
        if self.tables.declaration(&context.address(first)).is_some() {
            return None;
        }
        host_target(
            self.host,
            &path,
            Span::NONE,
            #[cfg(feature = "rust-import")]
            None,
        )
        .ok()?;
        Some(path)
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
        let package_name: ImmutableString = self.tables.package_name(package).into();
        let address = Address {
            package: package_name,
            modules: modules.into_boxed_slice(),
            name: name.clone(),
        };

        if let Some(declaration) = self.tables.declaration(&address) {
            self.validate_visibility(context, &address, declaration, reference)?;
            return Ok(declaration.target.clone());
        }
        if let Some(export) = self.tables.export(&address) {
            self.validate_module_hops(context, &address, reference)?;
            if !export.reach.reaches(&context.namespace) {
                return Err(self.private_import(context, &address, export, reference, active));
            }
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
        if let Some(alias) = self.tables.alias(&address) {
            self.validate_module_hops(context, &address, reference)?;
            if !alias.reach.reaches(&context.namespace) {
                return Err(self.private_import(context, &address, alias, reference, active));
            }
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
        if let Some(glob) = self.tables.glob(&address)
            && within(context, &address, address.modules.len())
            && active.insert(address.clone())
        {
            self.validate_module_hops(context, &address, reference)?;
            let target = self.resolve_any_path(&glob.context, &glob.path, glob.span, active)?;
            active.remove(&address);
            return Ok(target);
        }
        let from = base_modules.len();
        if let Some(target) =
            self.through_imported_module(context, &address, from, reference, active)
        {
            return target;
        }
        Err(self.unknown_import(context, path, reference))
    }

    /// `address` when one of its modules past the first `from` is an
    /// import of the module before it rather than a declaration there:
    /// `super::m::two` where the parent's `use crate::m;` binds `m` goes on
    /// from the module that import names, in the import's own module, as
    /// rustc resolves each segment through the scope of the one before it.
    /// `None` when every module is declared, or one is no import that
    /// reaches `context`.
    fn through_imported_module(
        &self,
        context: &Context,
        address: &Address,
        from: usize,
        reference: Span,
        active: &mut BTreeSet<Address>,
    ) -> Option<Result<Target, Failure>> {
        let modules = &address.modules;
        for at in from..modules.len() {
            let module = Address {
                package: address.package.clone(),
                modules: modules[..at].into(),
                name: modules[at].clone(),
            };
            if let Some(declaration) = self.tables.declaration(&module) {
                if matches!(declaration.target, Target::Module) {
                    continue;
                }
                return None;
            }
            let import = self
                .through_alias(&module)
                .or_else(|| self.tables.export(&module))
                .or_else(|| self.tables.glob(&module))
                .filter(|import| import.reach.reaches(&context.namespace))?;
            if !active.insert(module.clone()) {
                return None;
            }
            let expanded = import
                .path
                .iter()
                .chain(&modules[at + 1..])
                .chain(std::iter::once(&address.name))
                .cloned()
                .collect::<Vec<_>>();
            let target = self.resolve_any_path(&import.context, &expanded, reference, active);
            active.remove(&module);
            return Some(target);
        }
        None
    }

    fn validate_visibility(
        &self,
        context: &Context,
        address: &Address,
        declaration: &Declaration,
        reference: Span,
    ) -> Result<(), Failure> {
        self.validate_module_hops(context, address, reference)?;
        if !declaration.reach.reaches(&context.namespace) {
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
            let Some(module) = self.tables.declaration(&module_address) else {
                return Err(self.unknown_import(context, &[], reference));
            };
            if !module.reach.reaches(&context.namespace) {
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
        Failure {
            primary: context.source,
            secondary: Some(declaration.source),
            error: Error::PrivateItem {
                item: self.private_kind(&declaration.target),
                name: address.name.clone(),
                reference: last_segment(reference, &address.name),
                declaration: Some(declaration.span),
                detail: None,
            },
        }
    }

    /// rustc's E0603 for a path to `import`, a `use` binding at `address`
    /// that `context` is outside of the reach of: named by what it binds,
    /// as "function import `f` is private".
    fn private_import(
        &self,
        context: &Context,
        address: &Address,
        import: &Export,
        reference: Span,
        active: &mut BTreeSet<Address>,
    ) -> Failure {
        let mut imported = None;
        if active.insert(address.clone()) {
            // A std item is the kind std declares it as, as `Display` is a
            // trait, though Rawr holds it as a module.
            imported = self
                .import_spelling(&import.context, import.path.to_vec(), &mut BTreeSet::new())
                .and_then(|spelled| std_private(&spelled.join("::")))
                .or_else(|| {
                    self.resolve_any_path(&import.context, &import.path, import.span, active)
                        .ok()
                        .map(|target| self.private_kind(&target))
                })
                .and_then(Private::imported);
            active.remove(address);
        }
        Failure {
            primary: context.source,
            secondary: Some(import.source),
            error: Error::PrivateItem {
                item: imported.unwrap_or(Private::Import(super::Imported::Function)),
                name: address.name.clone(),
                reference: last_segment(reference, &address.name),
                declaration: Some(import.written),
                detail: self
                    .referent(context, import, &address.name, reference)
                    .map(|referent| Box::new(super::Detail::Import(referent))),
            },
        }
    }

    /// The crate item `import` binds as `bound`, when it is declared in
    /// the import's file, with whether `context` sees it, and the path
    /// rustc suggests in place of `written`, the path naming the import.
    fn referent(
        &self,
        context: &Context,
        import: &Export,
        bound: &ImmutableString,
        written: Span,
    ) -> Option<super::Referent> {
        let (origin, path) = self.origin(&import.context, &import.path);
        let (address, declaration) = self.declared_at(&origin, &path)?;
        let file = |source: source::Id| self.evidence[source.index()].identity();
        // rustc names the item by its own path when the use reaches it
        // by the import's name, and by the import's path otherwise.
        let instead = if *bound == address.name && self.sees(context, &address, &declaration.reach)
        {
            Some(super::Instead::Item(rustc_path(context, &address)))
        } else {
            let modules = import.path.split_last().map(|(_, modules)| modules);
            modules
                .filter(|modules| !modules.is_empty())
                .map(|modules| {
                    let segments = modules.iter().chain(std::iter::once(bound));
                    let path = segments.map(ImmutableString::as_str).collect::<Vec<_>>();
                    super::Instead::Import(path.join("::").into())
                })
        };
        (file(declaration.source) == file(import.source)).then(|| super::Referent {
            item: self.private_kind(&declaration.target),
            name: address.name.clone(),
            declaration: declaration.span,
            visible: declaration.reach.reaches(&context.namespace),
            instead,
            written,
        })
    }

    /// Whether `context` sees the item at `address`, which is visible
    /// where `reach` says: each module on its path, and the item itself.
    fn sees(&self, context: &Context, address: &Address, reach: &super::Reach) -> bool {
        reach.reaches(&context.namespace)
            && self
                .validate_module_hops(context, address, Span::NONE)
                .is_ok()
    }

    /// `failure`, a private module on the path `call` names from
    /// `context`, with the item the path goes on to, as rustc labels it:
    /// "function `f` is not publicly re-exported", or a public re-export
    /// of it the use could name.
    fn through_private(
        &self,
        context: &Context,
        call: &QualifiedCall,
        mut failure: Failure,
    ) -> Failure {
        let Error::PrivateItem {
            item: Private::Module,
            name,
            detail: detail @ None,
            ..
        } = &mut failure.error
        else {
            return failure;
        };
        let Some(outer) = call.path.last().filter(|outer| *outer != name) else {
            return failure;
        };
        let (origin, path) = self.origin(context, &call.path);
        let Some((address, declaration)) = self.declared_at(&origin, &path) else {
            return failure;
        };
        let item = self.used_kind(context, declaration, call).0;
        // A public `use` of the item the use sees, of the item's name.
        let reexport = self
            .tables
            .exports()
            .filter(|(at, export)| {
                at.name == *outer
                    && at.package == address.package
                    && self.sees(context, at, &export.reach)
            })
            .find(|(_, export)| {
                let mut active = BTreeSet::new();
                self.resolve_any_path(&export.context, &export.path, export.span, &mut active)
                    .is_ok_and(|target| target == declaration.target)
            })
            .map(|(at, _)| rustc_path(context, at));
        *detail = Some(Box::new(super::Detail::Through(super::Beyond {
            item,
            name: outer.clone(),
            reference: call.name_span,
            reexport,
            written: call.path_span,
        })));
        failure
    }

    /// What `declaration`, the item `call` names, is as rustc names it at
    /// that use: a struct named as a value is its constructor, a unit
    /// struct's or a tuple struct's, which comes with its fields.
    fn used_kind(
        &self,
        context: &Context,
        declaration: &Declaration,
        call: &QualifiedCall,
    ) -> (Private, Option<super::Fields>) {
        let item = self.private_kind(&declaration.target);
        let text = self.evidence[context.source.index()].text();
        if item != Private::Struct || !value_use(text, call.path_span) {
            return (item, None);
        }
        let declared = self
            .units
            .iter()
            .filter(|unit| unit.context.source == declaration.source)
            .find_map(|unit| {
                unit.parsed.items().iter().find_map(|parsed| match parsed {
                    crate::item::Parsed::Struct(declared)
                        if declared.name.span == declaration.span =>
                    {
                        Some((&unit.context, declared))
                    }
                    _ => None,
                })
            });
        let Some((declaring, declared)) = declared else {
            return (item, None);
        };
        match declared.form {
            crate::item::StructForm::Unit => (Private::UnitStruct, None),
            crate::item::StructForm::Tuple => {
                let opens = |field: &crate::item::Field| {
                    declaring
                        .namespace
                        .opens_to(field.visibility.as_ref(), &context.namespace)
                };
                let fields = declared
                    .fields
                    .first()
                    .zip(declared.fields.last())
                    .filter(|_| !declared.fields.iter().all(opens))
                    .map(|(first, last)| {
                        let start = first
                            .visibility
                            .as_ref()
                            .map_or(first.span, |visibility| visibility.span);
                        Span::new(start.start(), last.span.end())
                    });
                let private = declared
                    .fields
                    .iter()
                    .filter(|field| field.visibility.is_none() && !opens(field))
                    .map(|field| field.span.start())
                    .collect();
                let count = declared.fields.len();
                (
                    Private::TupleStructConstructor,
                    Some(super::Fields {
                        fields,
                        private,
                        count,
                    }),
                )
            }
            _ => (item, None),
        }
    }

    /// `failure`, a private struct `call` names from `context`, as rustc
    /// names it where the use is a value: a unit struct, or a tuple
    /// struct's constructor.
    fn private_value(
        &self,
        context: &Context,
        call: &QualifiedCall,
        mut failure: Failure,
    ) -> Failure {
        let Error::PrivateItem {
            item: item @ Private::Struct,
            name,
            declaration: Some(declared),
            detail: detail @ None,
            ..
        } = &mut failure.error
        else {
            return failure;
        };
        if call.path.last() != Some(name) {
            return failure;
        }
        let (origin, path) = self.origin(context, &call.path);
        let Some((_, declaration)) = self
            .declared_at(&origin, &path)
            .filter(|(_, declaration)| declaration.span == *declared)
        else {
            return failure;
        };
        let (used, fields) = self.used_kind(context, declaration, call);
        *item = used;
        *detail = fields.map(|fields| Box::new(super::Detail::Constructor(fields)));
        failure
    }

    /// What `target` is, as rustc names it in E0603.
    fn private_kind(&self, target: &Target) -> Private {
        match target {
            Target::Module => Private::Module,
            Target::Trait(_) => Private::Trait,
            Target::Type(type_ref::Target::Script { definition, .. })
                if self.tables.variants(definition).is_some() =>
            {
                Private::Enum
            }
            Target::Type(type_ref::Target::Alias(_)) => Private::TypeAlias,
            Target::Type(_) => Private::Struct,
            Target::Item(_) | Target::HostItem { .. } | Target::HostModuleItem { .. } => {
                Private::Function
            }
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

    /// rustc's E0433 for `path`, written at `written` in a body or a type,
    /// that `failure` refuses as an unresolved import where a module on the
    /// way has no module of the next segment's name: "cannot find `c` in
    /// `a`" at that segment, not the E0432 of an unresolved `use`. A miss
    /// of the last segment is the item's own error (`not_in_module`).
    fn missing_segment(
        &self,
        context: &Context,
        path: &[ImmutableString],
        segment_span: impl Fn(usize) -> Option<Span>,
        failure: Failure,
    ) -> Failure {
        let Error::UnknownImport {
            missing: Unresolved::Path,
            ..
        } = failure.error
        else {
            return failure;
        };
        let Some((end, reference)) = self
            .found_module(context, path)
            .filter(|end| end + 1 < path.len())
            .and_then(|end| Some((end, segment_span(end)?)))
        else {
            return failure;
        };
        let (segment, module) = (&path[end], &path[end - 1]);
        Failure {
            error: Error::StdPath {
                code: "E0433",
                message: format!("cannot find `{segment}` in `{module}`").into(),
                label: Some(format!("could not find `{segment}` in `{module}`").into()),
                note: None,
                reference,
            },
            ..failure
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

    /// Whether `qualified`, a dependency path a `use` in `context` names,
    /// is a miss against native code's retained facades rather than a
    /// macro: they hold no item there, and rustc hasn't resolved a macro
    /// there either.
    fn missed_macro_use(&self, context: &Context, qualified: &str) -> bool {
        #[cfg(feature = "rust-import")]
        return self.native.as_deref().is_some_and(|capture| {
            capture
                .at(program::source::Id::from_index(context.source.index()))
                .missing_binding(self.host, qualified)
        }) && self.tables.prelude().is_none_or(|prelude| {
            super::resolved_macro_use(prelude.project(), qualified).is_none()
        });
        #[cfg(not(feature = "rust-import"))]
        {
            let _ = (context, qualified);
            false
        }
    }

    /// The crates this package's paths may start with, as rustc's extern
    /// prelude has them. `None` for a host's one in-memory file, whose host
    /// decides what it reaches.
    fn prelude(&self) -> Option<super::extern_prelude::Prelude> {
        self.tables.prelude().cloned()
    }

    /// Whether `root`, a path's first segment in `context`, names no crate,
    /// module or item in scope, where rustc stops resolving the path.
    fn unresolved_root(&self, context: &Context, root: &str) -> bool {
        let address = context.address(root);
        // A `use` binds the name, unless it is `use root;` itself.
        let bound = |used: Option<&Export>| {
            used.is_some_and(|export| {
                export.path.first().map(ImmutableString::as_str) != Some(root)
            })
        };
        !matches!(
            root,
            "std" | "core" | "alloc" | "crate" | "self" | "super" | "Self"
        ) && self.tables.declaration(&address).is_none()
            && !bound(self.tables.export(&address))
            && !bound(self.tables.alias(&address))
            && !bound(self.tables.glob(&address))
            && self.host.entry(root).is_none()
            && !self.tables.reaches(root)
    }

    /// What `path` in `context` names in this crate or another. A probe: a
    /// crate's miss it finds isn't recorded.
    fn probe(&self, context: &Context, path: &[ImmutableString]) -> Option<Target> {
        if self.is_script_path(context, path) {
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
        }
    }

    /// Whether `path` in `context` names a module of this crate or of
    /// another.
    fn names_module(&self, context: &Context, path: &[ImmutableString]) -> bool {
        matches!(
            self.probe(context, path),
            Some(Target::Module | Target::HostModuleItem { .. })
        )
    }

    /// The kind of value `path` in `context` names, as rustc's E0432 names
    /// a value where the path needs a module or a type: `function` or
    /// `constant`.
    fn value_noun(&self, context: &Context, path: &[ImmutableString]) -> Option<&'static str> {
        match self.probe(context, path)? {
            Target::Item(key) if key.kind() == crate::item::Kind::Function => Some("function"),
            Target::HostItem { definitions, .. } => match definitions.first()?.kind() {
                host::item::Kind::Value => Some("constant"),
                host::item::Kind::Function | host::item::Kind::IntrinsicFunction => {
                    Some("function")
                }
                _ => None,
            },
            _ => None,
        }
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
                    imports: Box::default(),
                },
                None => return failure,
            },
            _ => match self.found_module(context, path) {
                Some(end) if end + 1 == path.len() => Error::UnknownImport {
                    path: path.join("::").into(),
                    reference,
                    missing: Unresolved::Item {
                        module: self.rustc_module(context, &path[..end]),
                        segment: reference,
                    },
                },
                Some(end) if let Some(span) = segment(end) => {
                    let module = self.rustc_module(context, &path[..end]);
                    Error::UnknownImport {
                        path: path[..=end].join("::").into(),
                        reference: span,
                        missing: match self.value_noun(context, &path[..=end]) {
                            Some(noun) => Unresolved::Value { noun, module },
                            None => Unresolved::Module(module),
                        },
                    }
                }
                _ => return failure,
            },
        };
        Failure { error, ..failure }
    }

    /// rustc's E0425 for the type path `path` into a module of this crate
    /// that `failure` refuses as an unresolved import: the module has no
    /// type of its last segment's name. E0405 where `noun` is `trait`, the
    /// path an impl's trait. So for a std module the path reaches through
    /// an import (`super::fmt::Missing` after `use std::fmt;`), which std's
    /// table refuses as having no item of the name.
    fn unresolved_type_path(
        &self,
        context: &Context,
        path: &crate::item::type_ref::Path,
        noun: &str,
        failure: Failure,
    ) -> Failure {
        let segments = path_segments(path);
        match (&failure.error, path.segments.last()) {
            (
                Error::UnknownImport {
                    reference,
                    missing: Unresolved::Path | Unresolved::Item { .. },
                    ..
                },
                Some(last),
            ) if *reference == path.span
                && self.found_module(context, &segments) == Some(segments.len() - 1) =>
            {
                if noun == "trait" {
                    let module = segments[..segments.len() - 1].join("::");
                    return Failure {
                        error: Error::StdPath {
                            code: "E0405",
                            message: format!(
                                "cannot find trait `{}` in module `{module}`",
                                last.name.text
                            )
                            .into(),
                            label: Some(format!("not found in `{module}`").into()),
                            note: None,
                            reference: last.name.span,
                        },
                        ..failure
                    };
                }
                Failure {
                    error: Error::UnknownType {
                        name: segments.join("::").into(),
                        reference: last.name.span,
                        path: false,
                        imports: Box::default(),
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
    /// its last. A path through a module the unit's `use`s bind, by
    /// `modules`, names the module's own path, as rustc resolves it, which
    /// a miss demands from the crate; the report writes the module as the
    /// source does.
    fn unresolved_type(
        &self,
        context: &Context,
        modules: &BTreeMap<ImmutableString, ImmutableString>,
        error: Error,
    ) -> Error {
        let error = self.unresolved_type_written(context, error);
        let Error::UnknownType {
            name,
            reference,
            path: false,
            imports,
        } = &error
        else {
            return error;
        };
        let Some(qualified) = name
            .split_once("::")
            .and_then(|(root, rest)| Some(format!("{}::{rest}", modules.get(root)?)))
        else {
            return error;
        };
        Error::UnknownType {
            name: qualified.into(),
            reference: *reference,
            path: false,
            imports: imports.clone(),
        }
    }

    /// [`Self::unresolved_type`]'s error at the path as it is written.
    fn unresolved_type_written(&self, context: &Context, error: Error) -> Error {
        let Error::UnknownType {
            name,
            reference,
            path: false,
            ..
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
                imports: Box::default(),
            };
        }
        let last_start = text_end(start, &name[..name.len() - last.len()]);
        Error::UnknownType {
            name: name.clone(),
            reference: Span::new(last_start, reference.end()),
            path: false,
            imports: Box::default(),
        }
    }
}

/// The span of `path`'s segment at `index`, where `written` is `path` as
/// written on one line with no blanks: `None` where it is written another
/// way.
fn written_segment(path: &[ImmutableString], written: Span, index: usize) -> Option<Span> {
    let start = written.start();
    if text_end(start, &path.join("::")) != written.end() {
        return None;
    }
    let segment = path.get(index)?;
    let before = path[..index]
        .iter()
        .map(|segment| segment.chars().count() + 2)
        .sum::<usize>();
    let mut at = start;
    for _ in 0..before {
        at.advance();
    }
    Some(Span::new(at, text_end(at, segment)))
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

/// The paths of more than one segment `body` calls or names.
fn qualified_calls(body: &crate::item::BodySyntax) -> Vec<QualifiedCall> {
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

            // `crate::name!` calls the program's `#[macro_export]` macro,
            // which the lowering expands where it is called.
            let exported_call = segments.len() == 2
                && segments[0].0.as_str() == "crate"
                && tokens
                    .get(end + 1)
                    .is_some_and(|token| matches!(token.kind(), crate::item::BodyTokenKind::Bang));
            if segments.len() >= 2 && !exported_call {
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
    collect(body, &mut output);
    output
}

/// The paths `body`'s patterns name a type through a module by, each with
/// its owner's span.
fn qualified_pattern_owners(body: &crate::item::BodySyntax) -> Vec<(Box<[ImmutableString]>, Span)> {
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
    collect(body, &mut output);
    output
}

/// The bodies of `unit`, whose paths it resolves: its items' functions',
/// and its statements' (#1141), lexed into `statements`.
fn unit_bodies<'a>(
    unit: &'a crate::parse::Unit,
    statements: &'a mut Option<crate::item::BodySyntax>,
) -> Vec<&'a crate::item::BodySyntax> {
    let (items, first, script) = unit.parts();
    *statements = first.map(|_| crate::parser::statement_syntax(script));
    let mut bodies = items
        .iter()
        .flat_map(|parsed| match parsed {
            crate::item::Parsed::Function(function) => std::slice::from_ref(function),
            crate::item::Parsed::Impl(crate::item::Impl { functions, .. })
            | crate::item::Parsed::Trait(crate::item::Trait { functions, .. }) => functions,
            crate::item::Parsed::Module(_)
            | crate::item::Parsed::Use(_)
            | crate::item::Parsed::Struct(_)
            | crate::item::Parsed::Enum(_)
            | crate::item::Parsed::TypeAlias(_) => &[],
        })
        .map(|function| &function.body_syntax)
        .collect::<Vec<_>>();
    bodies.extend(statements.as_ref());
    bodies
}

/// The path rustc suggests naming the item at `address` by from
/// `context`: its modules and name, through `crate::` outside the crate's
/// root.
fn rustc_path(context: &Context, address: &Address) -> ImmutableString {
    let root = context.namespace.modules.is_empty();
    let mut segments: Vec<&str> = Vec::with_capacity(address.modules.len() + 2);
    if !root {
        segments.push("crate");
    }
    segments.extend(address.modules.iter().map(ImmutableString::as_str));
    segments.push(&address.name);
    segments.join("::").into()
}

/// Whether the path at `path` in `text` names a value, as a call, an
/// operand or an argument does, and not a type, which follows a `:`, a
/// `->`, a `<` or an `as`, or comes before a struct literal's `{`.
fn value_use(text: &str, path: Span) -> bool {
    let line = |position: crate::Position| {
        let line = text.split('\n').nth(position.line()?.checked_sub(1)?)?;
        Some(line.chars().collect::<Vec<_>>())
    };
    let (Some(first), Some(last)) = (line(path.start()), line(path.end())) else {
        return true;
    };
    let start = path.start().position().unwrap_or(1).saturating_sub(1);
    let end = path.end().position().unwrap_or(1).saturating_sub(1);
    let after: String = last.iter().skip(end).collect();
    let after = after.trim_start();
    if after.starts_with('{') || after.starts_with('<') {
        return false;
    }
    let before: String = first.iter().take(start).collect();
    let mut before = before.trim_end();
    // A borrow of a type is a type too.
    while let Some(stripped) = before
        .strip_suffix("&mut")
        .or_else(|| before.strip_suffix('&'))
    {
        before = stripped.trim_end();
    }
    let typed = before.ends_with("->")
        || before.ends_with('<')
        || (before.ends_with(':') && !before.ends_with("::"))
        || before.ends_with(" as")
        || before.ends_with("impl")
        || before.ends_with("dyn");
    !typed
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

/// Whether code in the scope `here` sees a `use` of the scope `used`: the
/// module's everywhere, a block's in that block and the blocks nested in it.
fn sees(used: Option<Span>, here: Option<Span>) -> bool {
    match (used, here) {
        (None, _) => true,
        (Some(used), Some(here)) => super::block::encloses(used, here),
        (Some(_), None) => false,
    }
}

/// Declare `name` in `block` of `context`'s unit, as `target`: a second
/// item of that name in one block is rustc's E0428.
fn declare_in_block(
    blocks: &mut BlockItems,
    block: Span,
    context: &Context,
    name: &crate::item::Name,
    target: Target,
) -> Result<(), Failure> {
    let declared = blocks.entry(block).or_default();
    if let Some((previous, _)) = declared.get(&name.text) {
        return Err(Failure {
            primary: context.source,
            secondary: Some(context.source),
            error: Error::DuplicateItem {
                name: name.text.clone(),
                declaration: name.span,
                previous_declaration: *previous,
            },
        });
    }
    declared.insert(name.text.clone(), (name.span, target));
    Ok(())
}

/// The `pub` markers of `parsed`: its own, its impl's functions' and its
/// struct's fields'.
fn visibilities(parsed: &crate::item::Parsed) -> Vec<&crate::item::Visibility> {
    let own = header(parsed).and_then(|header| header.visibility.as_ref());
    let members: Vec<_> = match parsed {
        crate::item::Parsed::Impl(item) => item
            .functions
            .iter()
            .filter_map(|function| function.header.visibility.as_ref())
            .collect(),
        crate::item::Parsed::Struct(item) => item
            .fields
            .iter()
            .filter_map(|field| field.visibility.as_ref())
            .collect(),
        _ => Vec::new(),
    };
    own.into_iter().chain(members).collect()
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
                imports: Box::default(),
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
    let path = |path: &mut Path, _: Place| {
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

/// The path a signature names the module `qualified` a `use` binds by:
/// the std facade's module of a std module (`rawr_std::fmt`) is std's
/// (`std::fmt`), whose traits and types are std's own through it, as a
/// path written from std's root reaches them.
fn std_module(qualified: &ImmutableString) -> ImmutableString {
    qualified
        .strip_prefix(crate::std_facade::ROOT)
        .and_then(|rest| rest.strip_prefix("::"))
        .map(|rest| format!("std::{rest}"))
        .filter(|std| crate::std_table::is_module(std))
        .map_or_else(|| qualified.clone(), ImmutableString::from)
}

/// The kind of item the std path `path` names, as a privacy error names
/// it: `std::fmt::Display` is a trait. `None` for a module or a path std
/// has no item at.
fn std_private(path: &str) -> Option<Private> {
    let private = match crate::std_table::any(path)?.kind {
        "fn" => Private::Function,
        "struct" | "union" => Private::Struct,
        "enum" => Private::Enum,
        "trait" => Private::Trait,
        "type" => Private::TypeAlias,
        _ => return None,
    };
    Some(private)
}

/// Whether the prelude has a trait of `name`, which every module sees.
fn prelude_trait(name: &str) -> bool {
    crate::std_table::prelude(super::std_path::EDITION)
        .any(|path| path.kind == "trait" && path.path.rsplit("::").next() == Some(name))
}

/// Where a signature writes a path: as a type, or as a trait it bounds or
/// implements.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    /// A type: a field's, a parameter's, an alias's target or an impl's
    /// owner, at any depth of their arguments. A trait object's path
    /// (`dyn::Trait`) is one.
    Type,
    /// A trait a generic bound, an argument-position or return-position
    /// `impl Trait`, or an impl's header names.
    Trait,
}

/// Call `path` on each path of `item`'s signatures, with the place it is
/// written in: its types, at any depth of their arguments, its generic
/// bounds, an `impl Trait`'s and an impl's trait.
fn each_signature_path(
    item: &mut crate::item::Parsed,
    path: &impl Fn(&mut crate::item::type_ref::Path, Place),
) {
    use crate::item::type_ref::{Argument, Path, Type, IMPL_TYPE};
    fn ty(written: &mut Type, path: &impl Fn(&mut Path, Place)) {
        match written {
            Type::Unit(_) => {}
            Type::Tuple { members, .. } => {
                for member in members.iter_mut() {
                    ty(member, path);
                }
            }
            Type::Path(written) => {
                path(written, Place::Type);
                // `impl Bound + Bound` parses to the path `impl<Bound,
                // Bound>`: its arguments are traits.
                let opaque = matches!(&written.segments[..], [segment]
                    if segment.name.text == IMPL_TYPE);
                for segment in written.segments.iter_mut() {
                    for argument in segment.arguments.iter_mut() {
                        match argument {
                            Argument::Type(Type::Path(bound)) if opaque => {
                                path(bound, Place::Trait);
                                for segment in bound.segments.iter_mut() {
                                    for argument in segment.arguments.iter_mut() {
                                        if let Argument::Type(argument) = argument {
                                            ty(argument, path);
                                        }
                                    }
                                }
                            }
                            Argument::Type(argument) => ty(argument, path),
                            Argument::Lifetime(_) => {}
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
                path(bound, Place::Trait);
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
                path(trait_, Place::Trait);
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
