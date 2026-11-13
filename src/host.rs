//! Host metadata consulted while resolving Rawr source.

pub mod debug;
pub(crate) mod declared;
pub mod display;
mod error;
pub mod export;
pub mod facade;
#[cfg(feature = "rust-import")]
pub(crate) mod family;
#[cfg(feature = "rust-import")]
pub(crate) mod image;
pub mod item;
mod merge;
pub(crate) mod path;
pub(crate) mod projection;
pub mod runtime;
pub mod signature;
pub(crate) mod structural;
pub mod type_def;
pub mod type_ref;
pub mod usage;
pub mod value;

pub use error::Error;

use crate::tokenizer::is_valid_identifier;
use crate::ImmutableString;
use std::any::TypeId;

/// The name an exported field's items take beside the type's methods:
/// `Order::__rawr_field_items` for the field `items`.
pub(crate) const EXPORTED_FIELD: &str = "__rawr_field_";

const BUILTIN_TYPES: &[&str] = &[
    "bool", "char", "f64", "i64", "String", "i8", "i16", "i32", "isize", "u8", "u16", "u32", "u64",
    "usize", "f32", "i128", "u128",
];

/// Whether a re-export's `target` is a builtin type, as a facade's
/// `builtin_alias` names one: a single segment such as `String`.
pub(crate) fn is_builtin_alias_target(target: &path::Path) -> bool {
    matches!(target.segments(), [name] if BUILTIN_TYPES.contains(&name.as_str()))
}

/// Whether a re-export's `target` is an item of the standard library, as a
/// crate's re-export of one names it (`std_alias`): a path under `std`.
pub(crate) fn is_std_alias_target(target: &path::Path) -> bool {
    matches!(target.segments(), [root, _, ..] if root.as_str() == "std")
}

/// The runtime builtin a Rust numeric type is: every integer kind up to 64
/// bits an `i64`, `f32` an `f64` (the checker keeps the kinds apart), and
/// `i128` and `u128` an `i64` or, past its range, a boxed `i128` or `u128`.
pub(crate) fn runtime_builtin(name: &str) -> &str {
    match name {
        "i8" | "i16" | "i32" | "isize" | "u8" | "u16" | "u32" | "u64" | "usize" => "i64",
        "u128" => "i128",
        "f32" => "f64",
        name => name,
    }
}

#[cfg(feature = "rust-import")]
pub(crate) trait ImportAllocation {
    fn retain_checked(
        &mut self,
        items: Option<usize>,
        bytes: Option<usize>,
    ) -> Result<(), rawr_import::surface::Error>;

    /// Only private candidate construction supplies retained support ownership.
    fn support_owner(&self) -> Option<crate::package::import::runner::account::support::Owner> {
        None
    }
}

#[cfg(feature = "rust-import")]
impl ImportAllocation for rawr_import::contract::Allocation {
    fn retain_checked(
        &mut self,
        items: Option<usize>,
        bytes: Option<usize>,
    ) -> Result<(), rawr_import::surface::Error> {
        rawr_import::contract::Allocation::retain_checked(self, items, bytes)
    }
}

#[cfg(all(test, feature = "rust-import"))]
pub(crate) struct TestImportAllocation {
    items: usize,
    bytes: usize,
    item_limit: usize,
    byte_limit: usize,
}

#[cfg(all(test, feature = "rust-import"))]
impl Default for TestImportAllocation {
    fn default() -> Self {
        Self::new(usize::MAX, usize::MAX)
    }
}

#[cfg(all(test, feature = "rust-import"))]
impl TestImportAllocation {
    pub(crate) const fn new(item_limit: usize, byte_limit: usize) -> Self {
        Self {
            items: 0,
            bytes: 0,
            item_limit,
            byte_limit,
        }
    }

    pub(crate) const fn usage(&self) -> (usize, usize) {
        (self.items, self.bytes)
    }
}

#[cfg(all(test, feature = "rust-import"))]
impl ImportAllocation for TestImportAllocation {
    fn retain_checked(
        &mut self,
        items: Option<usize>,
        bytes: Option<usize>,
    ) -> Result<(), rawr_import::surface::Error> {
        let actual_items = self
            .items
            .checked_add(items.ok_or(rawr_import::surface::Error::Allocation)?)
            .ok_or(rawr_import::surface::Error::Allocation)?;
        let actual_bytes = self
            .bytes
            .checked_add(bytes.ok_or(rawr_import::surface::Error::Allocation)?)
            .ok_or(rawr_import::surface::Error::Allocation)?;
        if actual_items > self.item_limit || actual_bytes > self.byte_limit {
            return Err(rawr_import::surface::Error::Allocation);
        }
        self.items = actual_items;
        self.bytes = actual_bytes;
        Ok(())
    }
}

/// Static compiler metadata registered by the embedding host.
///
/// Runtime functions and script modules remain outside this registry.
#[derive(Clone, Debug)]
pub struct Registry {
    modules: Vec<path::Path>,
    types: Vec<type_def::Definition>,
    #[cfg(feature = "rust-import")]
    imported: Vec<ImportedIndex>,
    #[cfg(feature = "rust-import")]
    imported_aliases: Vec<usize>,
    #[cfg(feature = "rust-import")]
    structural: Vec<structural::Definition>,
    #[cfg(feature = "rust-import")]
    families: Vec<family::Pending>,
    /// Proof-backed applications, each by the key of the source call it
    /// answers.
    #[cfg(feature = "rust-import")]
    applied: Vec<Applied>,
    items: Vec<item::Definition>,
    facades: Vec<facade::Dependency>,
    /// Facade re-exports, sorted: a path and the canonical path it names.
    aliases: Vec<(path::Path, path::Path)>,
    /// The modules a facade reaches at another path, sorted: each path and
    /// the module's path its entries are below (`facade::Builder::
    /// imported_reexport`).
    reexports: Vec<(path::Path, path::Path)>,
    /// The same, sorted by the module's path, which a path's other
    /// spellings are found by (`Self::reexported`).
    reexported: Vec<(path::Path, path::Path)>,
    /// Crate type aliases, sorted by path (`iced::Result`).
    type_aliases: Vec<facade::TypeAlias>,
    /// The traits Rawr code names, sorted by path, and the crates' trait
    /// impls (D604).
    trait_headers: Vec<facade::TraitHeader>,
    impl_headers: Vec<facade::ImplHeader>,
    /// Where each facade's impl headers end in `impl_headers`, and its
    /// root, in the order they were registered: a header names its types
    /// as its own facade has them.
    impl_roots: Vec<(usize, ImmutableString)>,
    /// The crates' derive macros, sorted by path (`serde::Serialize`).
    derives: Vec<path::Path>,
    /// Roots an app exported with `#[rawr::export]`. Their types keep
    /// fields and methods apart, as Rust does.
    exported: Vec<ImmutableString>,
    /// The facades a runner's image holds (`image`), sorted by root: each
    /// builds an entry the first time a lookup here reaches it. Each keeps
    /// where its impl headers go among the others' (`impl_headers`), and
    /// how many held facades were installed before it.
    #[cfg(feature = "rust-import")]
    held: Vec<(image::Held, (usize, usize))>,
}

/// One proof-backed application's host method, owned by its receiver's facade.
#[cfg(feature = "rust-import")]
#[derive(Clone, Debug)]
pub(crate) struct Applied {
    key: [u8; 32],
    item: item::Definition,
    owner: facade::Dependency,
}

#[cfg(feature = "rust-import")]
#[derive(Clone, Debug)]
struct ImportedIndex {
    identity: type_def::import::Identity,
    facade: usize,
    aliases: std::ops::Range<usize>,
}

#[cfg(feature = "rust-import")]
pub(crate) const fn imported_index_bytes() -> usize {
    std::mem::size_of::<ImportedIndex>()
}

#[derive(Clone, Copy)]
pub(crate) enum Entry<'a> {
    Module,
    ModuleItem(&'a [item::Definition]),
    Type(&'a type_def::Definition),
    Item(&'a [item::Definition]),
    /// A facade's alias of a builtin type, as iced's `markdown::Uri` is
    /// `String`: the builtin's name.
    Builtin(&'a str),
    /// A crate's type alias, as `iced::Result` is.
    TypeAlias(&'a facade::TypeAlias),
}

#[cfg(feature = "rust-import")]
pub(crate) struct ImportedType<'a> {
    pub(crate) definition: &'a type_def::Definition,
    pub(crate) alias: Option<&'a type_def::Definition>,
    pub(crate) owner: Option<&'a facade::Dependency>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

/// The registry an engine holds. Each compile shares it as it stands
/// instead of copying it, so a compile costs what its program uses, not
/// what the engine registered: a universe runner's registry holds every
/// binding of its crates (#1065). A registration after a compile copies it
/// then, once, and the compile keeps the registry it read.
#[derive(Clone, Debug, Default)]
pub(crate) struct Current(std::sync::OnceLock<crate::Shared<Registry>>);

impl Current {
    /// An engine's registry before anything is registered.
    pub(crate) const fn new() -> Self {
        Self(std::sync::OnceLock::new())
    }

    /// The registry as it stands, for a compile to keep.
    pub(crate) fn share(&self) -> crate::Shared<Registry> {
        crate::Shared::clone(self.held())
    }

    fn held(&self) -> &crate::Shared<Registry> {
        self.0.get_or_init(|| crate::Shared::new(Registry::new()))
    }
}

impl std::ops::Deref for Current {
    type Target = Registry;

    fn deref(&self) -> &Registry {
        self.held()
    }
}

impl std::ops::DerefMut for Current {
    fn deref_mut(&mut self) -> &mut Registry {
        self.held();
        let held = self.0.get_mut().expect("the registry was just made");
        crate::Shared::make_mut(held)
    }
}

impl Registry {
    /// Create a registry containing Rawr's built-in header types.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            modules: Vec::new(),
            types: Vec::new(),
            #[cfg(feature = "rust-import")]
            imported: Vec::new(),
            #[cfg(feature = "rust-import")]
            imported_aliases: Vec::new(),
            #[cfg(feature = "rust-import")]
            structural: Vec::new(),
            #[cfg(feature = "rust-import")]
            families: Vec::new(),
            #[cfg(feature = "rust-import")]
            applied: Vec::new(),
            items: Vec::new(),
            facades: Vec::new(),
            aliases: Vec::new(),
            reexports: Vec::new(),
            reexported: Vec::new(),
            type_aliases: Vec::new(),
            trait_headers: Vec::new(),
            impl_headers: Vec::new(),
            impl_roots: Vec::new(),
            derives: Vec::new(),
            exported: Vec::new(),
            #[cfg(feature = "rust-import")]
            held: Vec::new(),
        }
    }

    /// Mark `root` as an app's exported items.
    pub(crate) fn mark_exported(&mut self, root: &str) {
        self.exported.push(root.into());
    }

    /// Whether the type or item at `path` is an app's export, whose fields
    /// and methods are apart.
    pub(crate) fn is_exported(&self, path: &str) -> bool {
        let root = path.split("::").next().unwrap_or(path);
        self.exported.iter().any(|exported| exported == root)
    }

    /// The facades installed, sorted by root.
    #[cfg(feature = "rust-import")]
    pub(crate) fn facades(&self) -> &[facade::Dependency] {
        &self.facades
    }

    /// The facades a runner's image holds, sorted by root.
    #[cfg(feature = "rust-import")]
    pub(crate) fn held_facades(&self) -> impl Iterator<Item = &image::Held> {
        self.held.iter().map(|(held, _)| held)
    }

    /// The Rust types its facades link, by their `TypeId`s.
    #[cfg(feature = "rust-import")]
    pub(crate) fn linked(&self) -> impl Iterator<Item = std::any::TypeId> + '_ {
        self.types
            .iter()
            .filter_map(type_def::Definition::native_identity)
    }

    /// Where an app exported the Rust type `identity`: its root and path.
    pub(crate) fn exported_path(&self, identity: std::any::TypeId) -> Option<String> {
        self.types
            .iter()
            .find(|definition| {
                definition.native_identity() == Some(identity)
                    && self.is_exported(&definition.path())
            })
            .map(type_def::Definition::path)
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn clone_for_import<A: ImportAllocation + ?Sized>(
        &self,
        allocation: &mut A,
    ) -> Result<Self, rawr_import::surface::Error> {
        let items = Some(1_usize)
            .and_then(|items| items.checked_add(self.modules.len()))
            .and_then(|items| items.checked_add(self.types.len()))
            .and_then(|items| items.checked_add(self.imported.len()))
            .and_then(|items| items.checked_add(self.imported_aliases.len()))
            .and_then(|items| items.checked_add(self.structural.len()))
            .and_then(|items| {
                self.families.iter().try_fold(items, |items, family| {
                    items.checked_add(1)?.checked_add(family.len())
                })
            })
            .and_then(|items| items.checked_add(self.applied.len()))
            .and_then(|items| items.checked_add(self.items.len()))
            .and_then(|items| items.checked_add(self.facades.len()));
        let mut bytes = Some(std::mem::size_of::<Self>());
        for path in &self.modules {
            bytes = bytes.and_then(|bytes| bytes.checked_add(path.retained_bytes()?));
        }
        for definition in &self.types {
            bytes = bytes.and_then(|bytes| bytes.checked_add(definition.retained_bytes()?));
        }
        bytes = bytes.and_then(|bytes| {
            bytes.checked_add(
                self.imported
                    .len()
                    .checked_mul(std::mem::size_of::<ImportedIndex>())?,
            )
        });
        bytes = bytes.and_then(|bytes| {
            bytes.checked_add(
                self.imported_aliases
                    .len()
                    .checked_mul(std::mem::size_of::<usize>())?,
            )
        });
        for definition in &self.structural {
            bytes = bytes.and_then(|bytes| bytes.checked_add(definition.retained_bytes()?));
        }
        for family in &self.families {
            bytes = bytes.and_then(|bytes| bytes.checked_add(family.retained_bytes()?));
        }
        for applied in &self.applied {
            bytes = bytes.and_then(|bytes| {
                bytes
                    .checked_add(std::mem::size_of::<Applied>())?
                    .checked_add(applied.item.retained_bytes()?)?
                    .checked_add(applied.owner.retained_bytes()?)
            });
        }
        for definition in &self.items {
            bytes = bytes.and_then(|bytes| bytes.checked_add(definition.retained_bytes()?));
        }
        for facade in &self.facades {
            bytes = bytes.and_then(|bytes| bytes.checked_add(facade.retained_bytes()?));
        }
        allocation.retain_checked(items, bytes)?;
        let mut clone = Self::new();
        clone
            .modules
            .try_reserve_exact(self.modules.len())
            .map_err(|_| rawr_import::surface::Error::Allocation)?;
        for path in &self.modules {
            clone.modules.push(path.try_clone()?);
        }
        clone
            .types
            .try_reserve_exact(self.types.len())
            .map_err(|_| rawr_import::surface::Error::Allocation)?;
        for definition in &self.types {
            clone.types.push(definition.try_clone()?);
        }
        clone
            .imported
            .try_reserve_exact(self.imported.len())
            .map_err(|_| rawr_import::surface::Error::Allocation)?;
        clone.imported.extend(self.imported.iter().cloned());
        clone
            .imported_aliases
            .try_reserve_exact(self.imported_aliases.len())
            .map_err(|_| rawr_import::surface::Error::Allocation)?;
        clone
            .imported_aliases
            .extend(self.imported_aliases.iter().copied());
        clone
            .structural
            .try_reserve_exact(self.structural.len())
            .map_err(|_| rawr_import::surface::Error::Allocation)?;
        clone.structural.extend(self.structural.iter().cloned());
        clone
            .families
            .try_reserve_exact(self.families.len())
            .map_err(|_| rawr_import::surface::Error::Allocation)?;
        for family in &self.families {
            clone.families.push(family.try_clone()?);
        }
        clone
            .applied
            .try_reserve_exact(self.applied.len())
            .map_err(|_| rawr_import::surface::Error::Allocation)?;
        for applied in &self.applied {
            clone.applied.push(Applied {
                key: applied.key,
                item: applied.item.try_clone()?,
                owner: applied.owner.try_clone()?,
            });
        }
        clone
            .items
            .try_reserve_exact(self.items.len())
            .map_err(|_| rawr_import::surface::Error::Allocation)?;
        for definition in &self.items {
            clone.items.push(definition.try_clone()?);
        }
        clone
            .facades
            .try_reserve_exact(self.facades.len())
            .map_err(|_| rawr_import::surface::Error::Allocation)?;
        for facade in &self.facades {
            clone.facades.push(facade.try_clone()?);
        }
        clone
            .aliases
            .try_reserve_exact(self.aliases.len())
            .map_err(|_| rawr_import::surface::Error::Allocation)?;
        for (alias, target) in &self.aliases {
            clone
                .aliases
                .push((alias.try_clone()?, target.try_clone()?));
        }
        for (reexports, cloned) in [
            (&self.reexports, &mut clone.reexports),
            (&self.reexported, &mut clone.reexported),
        ] {
            cloned
                .try_reserve_exact(reexports.len())
                .map_err(|_| rawr_import::surface::Error::Allocation)?;
            for (path, module) in reexports {
                cloned.push((path.try_clone()?, module.try_clone()?));
            }
        }
        clone
            .type_aliases
            .try_reserve_exact(self.type_aliases.len())
            .map_err(|_| rawr_import::surface::Error::Allocation)?;
        for alias in &self.type_aliases {
            clone.type_aliases.push(alias.try_clone()?);
        }
        clone
            .trait_headers
            .try_reserve_exact(self.trait_headers.len())
            .map_err(|_| rawr_import::surface::Error::Allocation)?;
        for header in &self.trait_headers {
            clone.trait_headers.push(header.try_clone()?);
        }
        clone
            .impl_headers
            .try_reserve_exact(self.impl_headers.len())
            .map_err(|_| rawr_import::surface::Error::Allocation)?;
        for header in &self.impl_headers {
            clone.impl_headers.push(header.try_clone()?);
        }
        clone.impl_roots.clone_from(&self.impl_roots);
        clone
            .derives
            .try_reserve_exact(self.derives.len())
            .map_err(|_| rawr_import::surface::Error::Allocation)?;
        for path in &self.derives {
            clone.derives.push(path.try_clone()?);
        }
        clone.exported.clone_from(&self.exported);
        // A held facade's entries are built once and shared by every copy.
        clone.held.clone_from(&self.held);
        Ok(clone)
    }

    /// Register a Rust type under a qualified path used in item headers.
    pub fn register_type<T: 'static>(
        &mut self,
        path: impl Into<ImmutableString>,
    ) -> Result<&mut Self, Error> {
        let path = path.into();

        if path.is_empty()
            || path
                .split("::")
                .any(|segment| !is_valid_identifier(segment))
        {
            return Err(Error::InvalidTypePath(path));
        }

        let canonical =
            path::Path::parse(path.clone()).map_err(|_| Error::InvalidTypePath(path))?;
        if self.owner_for_path(&canonical).is_some() {
            return Err(Error::RegistryConflict {
                path: canonical.to_string().into(),
                existing: self.entry_kind(&canonical).unwrap_or("binding"),
                incoming: "legacy type",
            });
        }
        if let Some(existing) = self.entry_kind(&canonical)
            && existing != "type"
        {
            return Err(Error::RegistryConflict {
                path: canonical.to_string().into(),
                existing,
                incoming: "legacy type",
            });
        }
        let index = self
            .types
            .binary_search_by(|definition| definition.path_ref().cmp(&canonical))
            .unwrap_or_else(|index| index);
        self.types.try_reserve(1).map_err(|_| Error::Allocation)?;
        let definition = type_def::Definition::legacy(canonical.clone(), TypeId::of::<T>());
        if index < self.types.len() && self.types[index].path_ref() == &canonical {
            self.types[index] = definition;
        } else {
            self.types.insert(index, definition);
            #[cfg(feature = "rust-import")]
            for alias in &mut self.imported_aliases {
                if *alias >= index {
                    *alias += 1;
                }
            }
        }
        Ok(self)
    }

    /// Atomically install a finished static facade proof.
    pub fn install(&mut self, definition: facade::Definition) -> Result<&mut Self, Error> {
        self.preflight(&definition)?;

        let module_count = definition
            .modules()
            .len()
            .checked_add(1)
            .ok_or(Error::Allocation)?;
        let type_count = definition.type_map().len();
        let item_count = definition.item_count();
        #[cfg(feature = "rust-import")]
        let imported_count = definition
            .type_map()
            .iter()
            .filter(|ty| ty.identity().imported().is_some())
            .count();
        #[cfg(feature = "rust-import")]
        let held = definition.held().cloned();
        self.modules
            .try_reserve(module_count)
            .map_err(|_| Error::Allocation)?;
        self.types
            .try_reserve(type_count)
            .map_err(|_| Error::Allocation)?;
        self.items
            .try_reserve(item_count)
            .map_err(|_| Error::Allocation)?;
        self.facades.try_reserve(1).map_err(|_| Error::Allocation)?;
        #[cfg(feature = "rust-import")]
        {
            self.imported
                .try_reserve(imported_count)
                .map_err(|_| Error::Allocation)?;
            self.imported_aliases
                .try_reserve(imported_count)
                .map_err(|_| Error::Allocation)?;
        }

        let parts = definition.into_registry_parts();
        let dependency = facade::Dependency::try_new(&parts.root, parts.fingerprint)?;
        // Merged into each sorted table: an entry already registered stays
        // ahead of an incoming one at its path, and only the incoming ones
        // are compared, so an install costs what its facade holds (#1755).
        let mut modules = Vec::from(parts.modules);
        modules.push(parts.root);
        merge::merge(&mut self.modules, modules, Ord::cmp);
        #[cfg_attr(not(feature = "rust-import"), allow(unused_variables))]
        let types = merge::merge(&mut self.types, parts.types.into_vec(), |left, right| {
            left.path_ref().cmp(right.path_ref())
        });
        merge::merge(&mut self.items, parts.items.into_vec(), |left, right| {
            left.path_ref().cmp(right.path_ref())
        });
        self.aliases
            .try_reserve(parts.aliases.len())
            .map_err(|_| Error::Allocation)?;
        merge::merge(&mut self.aliases, parts.aliases.into_vec(), Ord::cmp);
        self.reexports
            .try_reserve(parts.reexports.len())
            .map_err(|_| Error::Allocation)?;
        self.reexported
            .try_reserve(parts.reexports.len())
            .map_err(|_| Error::Allocation)?;
        merge::merge(
            &mut self.reexported,
            parts
                .reexports
                .iter()
                .map(|(path, module)| (module.clone(), path.clone()))
                .collect(),
            Ord::cmp,
        );
        merge::merge(&mut self.reexports, parts.reexports.into_vec(), Ord::cmp);
        self.type_aliases
            .try_reserve(parts.type_aliases.len())
            .map_err(|_| Error::Allocation)?;
        merge::merge(
            &mut self.type_aliases,
            parts.type_aliases.into_vec(),
            |left, right| left.path().cmp(right.path()),
        );
        self.trait_headers
            .try_reserve(parts.trait_headers.len())
            .map_err(|_| Error::Allocation)?;
        merge::merge(
            &mut self.trait_headers,
            parts.trait_headers.into_vec(),
            |left, right| left.path.cmp(&right.path),
        );
        self.impl_headers
            .try_reserve(parts.impl_headers.len())
            .map_err(|_| Error::Allocation)?;
        self.impl_headers.extend(parts.impl_headers);
        self.impl_roots
            .try_reserve(1)
            .map_err(|_| Error::Allocation)?;
        self.impl_roots
            .push((self.impl_headers.len(), dependency.root().into()));
        self.derives
            .try_reserve(parts.derives.len())
            .map_err(|_| Error::Allocation)?;
        merge::merge(&mut self.derives, parts.derives.into_vec(), Ord::cmp);
        let facade_index = self
            .facades
            .binary_search_by(|facade| facade.root().cmp(dependency.root()))
            .expect_err("registration preflight rejects occupied roots");
        self.facades.insert(facade_index, dependency);
        #[cfg(feature = "rust-import")]
        if let Some(held) = held {
            let index = self
                .held
                .binary_search_by(|(candidate, _)| candidate.root().cmp(held.root()))
                .expect_err("registration preflight rejects occupied roots");
            let installed = (self.impl_headers.len(), self.held.len());
            self.held.insert(index, (held, installed));
        }
        #[cfg(feature = "rust-import")]
        self.index_installed(&types, facade_index);
        Ok(self)
    }

    /// Every path the registry holds a module, type, item, family or
    /// re-export at, which a held registry's lookups are tested at.
    #[cfg(all(test, feature = "rust-import"))]
    pub(crate) fn every_path(&self) -> Vec<String> {
        let mut paths = self
            .modules
            .iter()
            .chain(self.types.iter().map(type_def::Definition::path_ref))
            .chain(self.items.iter().map(item::Definition::path_ref))
            .chain(self.families.iter().map(family::Pending::path))
            .chain(self.aliases.iter().map(|(alias, _)| alias))
            .chain(self.trait_headers.iter().map(|header| &header.path))
            .chain(self.derives.iter())
            .map(path::Path::to_string)
            .collect::<Vec<_>>();
        paths.sort();
        paths.dedup();
        paths
    }

    /// The crate type identities the registry's types have, and the trait
    /// identities of its impl headers.
    #[cfg(all(test, feature = "rust-import"))]
    pub(crate) fn every_identity(&self) -> (Vec<type_def::import::Identity>, Vec<[u8; 32]>) {
        let mut types = self
            .types
            .iter()
            .filter_map(|definition| definition.identity().imported().copied())
            .collect::<Vec<_>>();
        types.sort();
        types.dedup();
        let mut traits = self
            .impl_headers
            .iter()
            .map(|header| header.trait_identity)
            .collect::<Vec<_>>();
        traits.sort();
        traits.dedup();
        (types, traits)
    }

    /// The held facade whose root is `path`'s first segment.
    #[cfg(feature = "rust-import")]
    fn held(&self, path: &path::Path) -> Option<&image::Held> {
        let root = path.segments().first()?;
        self.held
            .binary_search_by(|(held, _)| held.root().name().cmp(root.as_str()))
            .ok()
            .map(|index| &self.held[index].0)
    }

    /// Whether a facade holds the module at `path`.
    fn has_module(&self, path: &path::Path) -> bool {
        if self.modules.binary_search(path).is_ok() {
            return true;
        }
        #[cfg(feature = "rust-import")]
        if let Some(held) = self.held(path) {
            return held.module(path);
        }
        false
    }

    /// The type at exactly `path`.
    fn type_at(&self, path: &path::Path) -> Option<&type_def::Definition> {
        if let Ok(index) = self
            .types
            .binary_search_by(|definition| definition.path_ref().cmp(path))
        {
            return Some(&self.types[index]);
        }
        #[cfg(feature = "rust-import")]
        if let Some(held) = self.held(path) {
            return held.type_index(path).map(|index| held.type_at(index));
        }
        None
    }

    /// Look up a registered type by its canonical qualified path.
    #[must_use]
    pub fn type_(&self, path: &str) -> Option<&type_def::Definition> {
        let path = path::Path::parse(path).ok()?;
        // Below a re-exported module, the type below the module it names.
        self.type_at(&path)
            .or_else(|| self.type_at(&self.through_reexports(&path)?))
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn structural(
        &self,
        path: &str,
        selector: &rawr_import::surface::identity::Selector,
        kind: structural::Kind,
        instance: Option<[u8; 32]>,
    ) -> Option<&structural::Definition> {
        let fingerprint = selector.fingerprint();
        // A variant's selector names its constructor and its matchers, and
        // a field's its reader and its store.
        let selected = |definition: &&structural::Definition| {
            definition.selector() == &fingerprint
                && definition.instance() == instance
                && if kind.shares_selector() || definition.kind().shares_selector() {
                    definition.kind() == kind
                } else {
                    true
                }
        };
        // One type at two paths has one adapter, at one of them (D536.1):
        // `ratatui::style::Color` is also `ratatui_core::style::Color`
        // through `ratatui::macros`.
        let identity = |path: &str| {
            self.type_(path)
                .and_then(|definition| definition.identity().imported())
                .cloned()
        };
        self.structural
            .iter()
            .filter(selected)
            .find(|definition| definition.path() == path)
            .or_else(|| {
                let wanted = identity(path)?;
                self.structural
                    .iter()
                    .filter(selected)
                    .find(|definition| identity(definition.path()).as_ref() == Some(&wanted))
            })
    }

    /// Whether the runner admitted the adapter `definition`.
    #[cfg(feature = "rust-import")]
    pub(crate) fn admits_structural(&self, definition: &structural::Definition) -> bool {
        // Sorted by this key, which admits each once (`admit_structural`).
        let key = (
            definition.owner(),
            definition.path(),
            definition.selector(),
            definition.kind(),
            definition.instance(),
        );
        self.structural
            .binary_search_by(|candidate| {
                (
                    candidate.owner(),
                    candidate.path(),
                    candidate.selector(),
                    candidate.kind(),
                    candidate.instance(),
                )
                    .cmp(&key)
            })
            .is_ok_and(|index| self.structural[index] == *definition)
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn admit_structural<A: ImportAllocation + ?Sized>(
        &mut self,
        definition: structural::Definition,
        allocation: &mut A,
    ) -> Result<(), rawr_import::surface::Error> {
        let key = (
            definition.owner(),
            definition.path(),
            definition.selector(),
            definition.kind(),
            definition.instance(),
        );
        let index = match self.structural.binary_search_by(|candidate| {
            (
                candidate.owner(),
                candidate.path(),
                candidate.selector(),
                candidate.kind(),
                candidate.instance(),
            )
                .cmp(&key)
        }) {
            Ok(_) => {
                return Err(rawr_import::surface::Error::AdapterPlan {
                    reason: "duplicate generated structural admission",
                });
            }
            Err(index) => index,
        };
        allocation.retain_checked(Some(1), definition.retained_bytes())?;
        self.structural
            .try_reserve(1)
            .map_err(|_| rawr_import::surface::Error::Allocation)?;
        self.structural.insert(index, definition);
        Ok(())
    }

    /// A public type source names by `path`, also through a facade
    /// re-export (`iced::Renderer`), as Rust names a re-exported type.
    pub(crate) fn source_type(&self, path: &str) -> Option<&type_def::Definition> {
        self.type_by_path(&path::Path::parse(path).ok()?)
            .filter(|definition| definition.visibility() == type_def::Visibility::Public)
    }

    /// The one path of the crate type at `path`, whatever route `path`
    /// takes to it: its first in its facade, the one its signatures name
    /// it by. `iced::theme::Mode` and `iced::advanced::graphics::core::
    /// theme::Mode` are one type with one such path. `None` when `path`
    /// names no crate type.
    #[cfg(feature = "rust-import")]
    pub(crate) fn imported_type_path(&self, path: &str) -> Option<&path::Path> {
        let identity = self.type_(path)?.identity().imported()?;
        let owner = self.type_owner(path)?;
        Some(
            self.imported_type(identity, owner, None)?
                .definition
                .path_ref(),
        )
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn imported_type(
        &self,
        identity: &type_def::import::Identity,
        owner: &facade::Dependency,
        originating: Option<&type_def::Definition>,
    ) -> Option<ImportedType<'_>> {
        let facade = self
            .facades
            .binary_search_by(|candidate| candidate.root().cmp(owner.root()))
            .ok()?;
        if &self.facades[facade] != owner {
            return None;
        }
        if let Some(held) = self.held(&path::Path::parse(owner.root()).ok()?) {
            let aliases = held.identity_types(identity);
            let first = *aliases.first()?;
            let originating =
                originating.filter(|definition| definition.identity().imported() == Some(identity));
            let exact = originating.and_then(|originating| {
                aliases
                    .iter()
                    .copied()
                    .find(|alias| &held.type_path(*alias) == originating.path_ref())
            });
            // A type below a re-exported module has its paths below each
            // module that re-exports it too.
            let exact_alias = exact.is_some()
                || aliases.len() == 1 && self.reexported(&held.type_path(first)).is_empty();
            let definition = held.type_at(exact.unwrap_or(first));
            return Some(ImportedType {
                definition,
                alias: exact_alias.then_some(definition),
                owner: Some(&self.facades[facade]),
            });
        }
        let index = self
            .imported
            .binary_search_by(|candidate| {
                candidate
                    .identity
                    .cmp(identity)
                    .then_with(|| candidate.facade.cmp(&facade))
            })
            .ok()?;
        let aliases = &self.imported_aliases[self.imported[index].aliases.clone()];
        let originating =
            originating.filter(|definition| definition.identity().imported() == Some(identity));
        let exact = originating.and_then(|originating| {
            aliases
                .iter()
                .copied()
                .find(|alias| self.types[*alias].path() == originating.path())
        });
        // A type below a re-exported module has its paths below each
        // module that re-exports it too.
        let exact_alias = exact.is_some()
            || aliases.len() == 1
                && self
                    .reexported(self.types[aliases[0]].path_ref())
                    .is_empty();
        let alias = exact.unwrap_or(aliases[0]);
        let definition = &self.types[alias];
        Some(ImportedType {
            definition,
            alias: exact_alias.then_some(definition),
            owner: Some(&self.facades[facade]),
        })
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn imported_type_paths<'a>(
        &'a self,
        identity: &type_def::import::Identity,
        owner: &facade::Dependency,
    ) -> Option<impl ExactSizeIterator<Item = String> + 'a> {
        let facade = self
            .facades
            .binary_search_by(|candidate| candidate.root().cmp(owner.root()))
            .ok()?;
        if &self.facades[facade] != owner {
            return None;
        }
        if let Some(held) = self.held(&path::Path::parse(owner.root()).ok()?) {
            let aliases = held.identity_types(identity);
            if aliases.is_empty() {
                return None;
            }
            return Some(
                self.type_paths(aliases.into_iter().map(|alias| held.type_path(alias)))
                    .into_iter(),
            );
        }
        let imported = self
            .imported
            .binary_search_by(|candidate| {
                candidate
                    .identity
                    .cmp(identity)
                    .then_with(|| candidate.facade.cmp(&facade))
            })
            .ok()?;
        Some(
            self.type_paths(
                self.imported_aliases[self.imported[imported].aliases.clone()]
                    .iter()
                    .map(|alias| self.types[*alias].path_ref().clone()),
            )
            .into_iter(),
        )
    }

    /// The paths of the types at `aliases` and each other path a
    /// re-exported module gives them, in the order a type's paths take: a
    /// crate's hidden segment after its public paths, then path order.
    #[cfg(feature = "rust-import")]
    fn type_paths(&self, aliases: impl Iterator<Item = path::Path>) -> Vec<String> {
        let mut paths = Vec::new();
        for path in aliases {
            let reexported = self.reexported(&path);
            paths.push(path);
            paths.extend(reexported);
        }
        let hidden = |path: &path::Path| {
            path.segments()
                .iter()
                .any(|segment| path::is_hidden(segment))
        };
        paths.sort_by(|left, right| (hidden(left), left).cmp(&(hidden(right), right)));
        paths.iter().map(path::Path::to_string).collect()
    }

    /// Every path of the imported type `identity` in any facade: another
    /// crate's facade names it under that crate's hidden segment
    /// (`natives::__rawr_crate_parts_<id>::Terminal`) when a trait of that
    /// crate gives it members (#1450).
    #[cfg(feature = "rust-import")]
    pub(crate) fn imported_type_paths_anywhere<'a>(
        &'a self,
        identity: &type_def::import::Identity,
    ) -> impl Iterator<Item = String> + 'a {
        let start = self
            .imported
            .partition_point(|candidate| candidate.identity < *identity);
        let end = self
            .imported
            .partition_point(|candidate| candidate.identity <= *identity);
        let eager = &self.imported[start..end];
        let identity = *identity;
        // In facade order: each facade's paths of the type, held or not.
        self.facades
            .iter()
            .enumerate()
            .flat_map(move |(facade, dependency)| {
                if let Some(held) = path::Path::parse(dependency.root())
                    .ok()
                    .and_then(|root| self.held(&root))
                {
                    return self.type_paths(
                        held.identity_types(&identity)
                            .into_iter()
                            .map(|alias| held.type_path(alias)),
                    );
                }
                eager
                    .iter()
                    .filter(|imported| imported.facade == facade)
                    .flat_map(|imported| {
                        self.type_paths(
                            self.imported_aliases[imported.aliases.clone()]
                                .iter()
                                .map(|alias| self.types[*alias].path_ref().clone()),
                        )
                    })
                    .collect()
            })
    }

    /// Look up a registered static item by its canonical qualified path.
    #[must_use]
    pub fn item(&self, path: &str) -> Option<&item::Definition> {
        let path = path::Path::parse(path).ok()?;
        let items = self
            .item_slice(&path)
            .or_else(|| self.item_slice(&self.through_reexports(&path)?))?;
        let [item] = items else {
            return None;
        };
        Some(item)
    }

    /// The paths of the free functions and macros named `name`, in any
    /// module: `iced::widget::center` for `center`.
    pub(crate) fn functions_named<'a>(
        &'a self,
        name: &'a str,
    ) -> impl Iterator<Item = String> + 'a {
        #[allow(unused_mut)]
        let mut functions = self
            .items
            .iter()
            .filter(move |item| {
                item.path_ref().name() == name
                    && matches!(
                        item.kind(),
                        item::Kind::Function | item::Kind::IntrinsicFunction
                    )
            })
            .collect::<Vec<_>>();
        #[cfg(feature = "rust-import")]
        {
            for (held, _) in &self.held {
                functions.extend(held.functions_named(name));
            }
            functions.sort_by(|left, right| left.path_ref().cmp(right.path_ref()));
        }
        // And at each path a re-exported module gives it.
        functions.into_iter().flat_map(|item| {
            let path = item.path_ref();
            std::iter::once(path.to_string())
                .chain(self.reexported(path).iter().map(path::Path::to_string))
                .collect::<Vec<_>>()
        })
    }

    /// Items declared directly under `owner`, such as a type's methods.
    /// Below a re-exported module, those under the owner below the module
    /// it names.
    pub(crate) fn members<'a>(&'a self, owner: &str) -> impl Iterator<Item = &'a item::Definition> {
        let owner = path::Path::parse(owner)
            .ok()
            .map(|owner| self.through_reexports(&owner).unwrap_or(owner));
        #[cfg(feature = "rust-import")]
        if let Some(held) = owner.as_ref().and_then(|owner| self.held(owner)) {
            return held
                .members(owner.as_ref().expect("a held facade's owner parsed"))
                .into_iter();
        }
        self.items
            .iter()
            .filter(move |item| owner.is_some() && item.path_ref().parent() == owner)
            .collect::<Vec<_>>()
            .into_iter()
    }

    pub(crate) fn overloads(&self, path: &str) -> Option<&[item::Definition]> {
        let path = path::Path::parse(path).ok()?;
        // `canvas::stroke::Style::Solid` through the re-export of `Style`.
        self.item_slice(&path)
            .or_else(|| self.item_slice(&self.canonical(&path)?))
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn defer_imported(&mut self, families: Vec<family::Pending>) -> Result<(), Error> {
        for family in &families {
            // Families stay sorted by path, so a prior one is found by search.
            if self.item_slice(family.path()).is_some()
                || self
                    .families
                    .binary_search_by(|prior| prior.path().cmp(family.path()))
                    .is_ok()
            {
                return Err(Error::DuplicateFacadeOverloadInputs {
                    item: family.path().to_string().into(),
                });
            }
        }
        self.families
            .try_reserve_exact(families.len())
            .map_err(|_| Error::Allocation)?;
        self.families.extend(families);
        self.families
            .sort_by(|left, right| left.path().cmp(right.path()));
        Ok(())
    }

    /// Admit one proof-backed application's method, owned by the facade of
    /// `root`. Its key is the source call's; one key has one application.
    #[cfg(feature = "rust-import")]
    pub(crate) fn admit_applied(
        &mut self,
        key: [u8; 32],
        root: &str,
        item: item::Definition,
    ) -> Result<(), Error> {
        let owner = self
            .facades
            .iter()
            .find(|facade| facade.root() == root)
            .cloned()
            .ok_or_else(|| Error::MissingFacadeModule {
                path: item.path().to_string().into(),
                module: root.into(),
            })?;
        match self
            .applied
            .binary_search_by(|applied| applied.key.cmp(&key))
        {
            Ok(index) if self.applied[index].item.fingerprint() == item.fingerprint() => Ok(()),
            Ok(_) => Err(Error::DuplicateFacadeOverloadInputs {
                item: item.path().to_string().into(),
            }),
            Err(index) => {
                self.applied.try_reserve(1).map_err(|_| Error::Allocation)?;
                self.applied.insert(index, Applied { key, item, owner });
                Ok(())
            }
        }
    }

    /// Each proof-backed application it admits and the key of the source
    /// call it answers, by that key.
    #[cfg(feature = "rust-import")]
    pub(crate) fn applications(&self) -> impl Iterator<Item = (&[u8; 32], &item::Definition)> {
        self.applied
            .iter()
            .map(|applied| (&applied.key, &applied.item))
    }

    /// The proof-backed application answering the source call `key`.
    #[cfg(feature = "rust-import")]
    pub(crate) fn applied(
        &self,
        key: &[u8; 32],
    ) -> Option<(&item::Definition, &facade::Dependency)> {
        self.applied
            .binary_search_by(|applied| applied.key.cmp(key))
            .ok()
            .map(|index| (&self.applied[index].item, &self.applied[index].owner))
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn pending_family(&self, qualified: &str) -> Option<&family::Pending> {
        let path = path::Path::parse(qualified).ok()?;
        let find = |path: &path::Path| {
            self.families
                .binary_search_by(|family| family.path().cmp(path))
                .ok()
                .map(|index| &self.families[index])
                .or_else(|| self.held(path)?.family(path))
        };
        find(&path).or_else(|| find(&self.canonical(&path)?))
    }

    /// Return the exact facade owner of a registered type.
    ///
    /// Legacy direct type registrations return `None`.
    #[must_use]
    pub fn type_owner(&self, path: &str) -> Option<&facade::Dependency> {
        let path = path::Path::parse(path).ok()?;
        self.owner_for_path(&path)
    }

    pub(crate) fn trait_types(
        &self,
    ) -> impl Iterator<Item = (&type_def::Definition, &facade::Dependency)> {
        self.types.iter().filter_map(|definition| {
            (definition.role() == type_def::Role::Trait)
                .then(|| {
                    self.owner_for_path(definition.path_ref())
                        .map(|owner| (definition, owner))
                })
                .flatten()
        })
    }

    /// Return the exact facade owner of a registered item.
    #[must_use]
    pub fn item_owner(&self, path: &str) -> Option<&facade::Dependency> {
        let path = path::Path::parse(path).ok()?;
        self.owner_for_path(&path)
    }

    /// Return the exact proof for an installed facade root.
    #[must_use]
    pub fn facade_fingerprint(&self, root: &str) -> Option<&facade::Fingerprint> {
        let root = path::Path::parse(root).ok()?;
        self.facades
            .binary_search_by(|facade| facade.root().cmp(root.name()))
            .ok()
            .map(|index| self.facades[index].fingerprint())
    }

    pub(crate) fn is_builtin(&self, path: &str) -> bool {
        BUILTIN_TYPES.contains(&path)
    }

    pub(crate) fn entry(&self, qualified: &str) -> Option<Entry<'_>> {
        let path = path::Path::parse(qualified).ok()?;
        // Below a re-exported module, what lies below the module it names.
        self.entry_at(&path)
            .or_else(|| self.entry_at(&self.through_reexports(&path)?))
    }

    fn entry_at(&self, path: &path::Path) -> Option<Entry<'_>> {
        if self.has_module(path) {
            Some(match self.item_slice(path) {
                Some(items) => Entry::ModuleItem(items),
                None => Entry::Module,
            })
        } else if let Some(ty) = self.type_by_path(path) {
            (ty.visibility() == type_def::Visibility::Public).then_some(Entry::Type(ty))
        } else if let Some(builtin) = self.builtin_alias(path) {
            Some(Entry::Builtin(builtin))
        } else if let Some(alias) = self.type_alias(path) {
            Some(Entry::TypeAlias(alias))
        } else {
            if path
                .parent()
                .and_then(|parent| self.type_by_path(&parent))
                .is_some_and(|owner| owner.visibility() == type_def::Visibility::Private)
            {
                return None;
            }
            self.item_slice(path).map(Entry::Item)
        }
    }

    /// The names a glob import of `module` binds: each module, type and item
    /// directly inside it that resolves as an import, sorted and distinct.
    pub(crate) fn children(&self, module: &str) -> Vec<ImmutableString> {
        let Ok(module) = path::Path::parse(module) else {
            return Vec::new();
        };
        let mut names = self.inside(&module);
        // A re-exported module's are those of the module it names.
        if let Some(reexported) = self.through_reexports(&module) {
            names.extend(self.inside(&reexported));
            names.sort_unstable();
            names.dedup();
        }
        names
    }

    fn inside(&self, module: &path::Path) -> Vec<ImmutableString> {
        let inside = |path: &path::Path| {
            path.segments().len() == module.segments().len() + 1
                && path.segments().starts_with(module.segments())
        };
        #[allow(unused_mut)]
        let mut candidates = self
            .modules
            .iter()
            .chain(self.types.iter().map(type_def::Definition::path_ref))
            .chain(self.items.iter().map(item::Definition::path_ref))
            .chain(
                self.aliases
                    .iter()
                    .filter(|(_, target)| is_builtin_alias_target(target))
                    .map(|(alias, _)| alias),
            )
            .chain(self.type_aliases.iter().map(facade::TypeAlias::path))
            .chain(
                self.aliases
                    .iter()
                    .filter(|(alias, _)| inside(alias))
                    .filter(|(alias, _)| self.enum_variant(&alias.to_string()).is_some())
                    .map(|(alias, _)| alias),
            )
            .filter(|path| inside(path))
            .cloned()
            .collect::<Vec<_>>();
        #[cfg(feature = "rust-import")]
        if let Some(held) = self.held(module) {
            let held = held.inside(module);
            candidates.extend(held.modules);
            candidates.extend(held.types);
            candidates.extend(held.items);
            candidates.extend(held.aliases.into_iter().filter_map(|(alias, kind)| {
                (kind == image::Kind::Builtin || self.enum_variant(&alias.to_string()).is_some())
                    .then_some(alias)
            }));
        }
        let mut names = candidates
            .iter()
            .filter(|path| {
                let qualified = path.segments().join("::");
                self.entry(&qualified).is_some() || self.enum_variant(&qualified).is_some()
            })
            .map(|path| ImmutableString::from(path.name()))
            .collect::<Vec<_>>();
        names.sort_unstable();
        names.dedup();
        names
    }

    fn preflight(&self, definition: &facade::Definition) -> Result<(), Error> {
        let root = definition.root_path();
        if self
            .facades
            .binary_search_by(|facade| facade.root().cmp(root.name()))
            .is_ok()
            || self.entry_kind(root).is_some()
        {
            return Err(Error::RegistryConflict {
                path: root.to_string().into(),
                existing: self.entry_kind(root).unwrap_or("bindings"),
                incoming: "bindings",
            });
        }

        for module in definition.modules() {
            self.ensure_registry_vacant(module, "module")?;
        }
        for incoming in definition.type_map() {
            let path = incoming.path_ref();
            self.ensure_registry_vacant(path, "type")?;
            for projection in incoming.projections() {
                let Some(target) = projection.external_target() else {
                    continue;
                };
                let Some(installed_facade) = self.facade_fingerprint_path(target.facade_root())
                else {
                    return Err(Error::MissingFacadeDependency {
                        source: incoming.path().into(),
                        facade: target.facade_root().to_string().into(),
                    });
                };
                let target_definition = self.type_by_path(target.contract().path_ref());
                if installed_facade != target.facade()
                    || target_definition
                        .map(type_def::Definition::contract)
                        .as_ref()
                        != Some(target.contract())
                {
                    return Err(Error::FacadeDependencyMismatch {
                        source: incoming.path().into(),
                        target: target.path().into(),
                    });
                }
            }
            if let Some(existing) = self.type_with_identity(incoming.identity())
                && !existing.same_identity_contract(incoming)
            {
                return Err(Error::ConflictingHostIdentity {
                    existing: existing.path().into(),
                    incoming: incoming.path().into(),
                });
            }
        }
        for (path, items) in definition.item_sets() {
            self.ensure_registry_vacant(path, "item")?;
            for item in items {
                for export in item.signature().external_types() {
                    self.validate_export(item.path(), export)?;
                }
            }
        }
        Ok(())
    }

    fn validate_export(&self, source: String, export: &type_def::Export) -> Result<(), Error> {
        let Some(installed_facade) = self.facade_fingerprint_path(export.facade_root()) else {
            return Err(Error::MissingFacadeDependency {
                source: source.into(),
                facade: export.facade_root().to_string().into(),
            });
        };
        let target_definition = self.type_by_path(export.contract().path_ref());
        if installed_facade != export.facade()
            || target_definition
                .map(type_def::Definition::contract)
                .as_ref()
                != Some(export.contract())
        {
            return Err(Error::FacadeDependencyMismatch {
                source: source.into(),
                target: export.path().into(),
            });
        }
        Ok(())
    }

    fn ensure_registry_vacant(
        &self,
        path: &path::Path,
        incoming: &'static str,
    ) -> Result<(), Error> {
        if let Some(existing) = self.entry_kind(path) {
            return Err(Error::RegistryConflict {
                path: path.to_string().into(),
                existing,
                incoming,
            });
        }
        Ok(())
    }

    fn entry_kind(&self, path: &path::Path) -> Option<&'static str> {
        if self.has_module(path) {
            Some("module")
        } else if self.type_at(path).is_some() {
            Some("type")
        } else if self.item_slice(path).is_some() {
            Some("item")
        } else {
            None
        }
    }

    fn type_by_path(&self, path: &path::Path) -> Option<&type_def::Definition> {
        self.type_at(path)
            .or_else(|| self.type_at(&self.canonical(path)?))
    }

    /// The enum whose variant `qualified` names, as `Enum::Variant` or
    /// through a facade's re-export of the variant: `iced::Center` is
    /// `iced::Alignment::Center`. The enum's path, as the facade spells it.
    pub(crate) fn enum_variant(&self, qualified: &str) -> Option<ImmutableString> {
        let path = path::Path::parse(qualified).ok()?;
        let path = self.canonical(&path).unwrap_or(path);
        let owner = path.parent()?;
        let ty = self.type_by_path(&owner)?;
        if ty.visibility() != type_def::Visibility::Public {
            return None;
        }
        let type_def::Shape::Enum { variants, .. } = ty.shape() else {
            return None;
        };
        variants
            .iter()
            .any(|variant| variant.name() == path.name())
            .then(|| ty.path_ref().to_string().into())
    }

    /// The variants of the public crate enum `qualified` names, as a glob
    /// import of it (`use iced::Alignment::*;`) reaches them.
    pub(crate) fn enum_variants(&self, qualified: &str) -> Option<Vec<ImmutableString>> {
        let path = path::Path::parse(qualified).ok()?;
        let path = self.canonical(&path).unwrap_or(path);
        let ty = self.type_by_path(&path)?;
        if ty.visibility() != type_def::Visibility::Public {
            return None;
        }
        let type_def::Shape::Enum { variants, .. } = ty.shape() else {
            return None;
        };
        Some(
            variants
                .iter()
                .map(|variant| ImmutableString::from(variant.name()))
                .collect(),
        )
    }

    /// A crate type alias that passes its own parameters on, all and in
    /// order, to the imported type `identity`, as iced's `canvas::Cache<
    /// Renderer = ..>` does to `geometry::Cache<Renderer>`. Its path reaches
    /// the type's associated functions when the type has no path of its own.
    #[cfg(feature = "rust-import")]
    pub(crate) fn passing_alias(
        &self,
        identity: &type_def::import::Identity,
    ) -> Option<&facade::TypeAlias> {
        self.type_aliases.iter().find(|alias| {
            matches!(
                alias.target().shape(),
                type_ref::Shape::Imported { identity: target, arguments }
                    if target == identity
                        && arguments.len() == alias.parameters().len()
                        && arguments.iter().zip(alias.parameters()).all(|(argument, parameter)| {
                            matches!(
                                argument.shape(),
                                type_ref::Shape::Parameter(name)
                                    if name.to_string() == parameter.as_str()
                            )
                        })
            )
        })
    }

    /// The crate type alias at `path` (`Entry::TypeAlias`), or at the path
    /// a facade re-export of it names.
    pub(crate) fn type_alias(&self, path: &path::Path) -> Option<&facade::TypeAlias> {
        let found = |path: &path::Path| {
            self.type_aliases
                .binary_search_by(|alias| alias.path().cmp(path))
                .ok()
                .map(|index| &self.type_aliases[index])
        };
        found(path).or_else(|| found(&self.canonical(path)?))
    }

    /// Whether `qualified` names a crate's derive macro, as
    /// `serde::Serialize`, directly or through a facade re-export.
    pub(crate) fn derive_macro(&self, qualified: &str) -> bool {
        let Ok(path) = path::Path::parse(qualified) else {
            return false;
        };
        let found = |path: &path::Path| {
            let found = self.derives.binary_search(path).is_ok();
            #[cfg(feature = "rust-import")]
            let found = found || self.held(path).is_some_and(|held| held.derive(path));
            found
        };
        found(&path) || self.canonical(&path).is_some_and(|path| found(&path))
    }

    #[cfg(feature = "rust-import")]
    /// The header of the crate trait at `path`, or at the path a facade
    /// re-export of it names (D604).
    pub(crate) fn trait_header(&self, path: &path::Path) -> Option<&facade::TraitHeader> {
        let found = |path: &path::Path| {
            self.trait_headers
                .binary_search_by(|header| header.path.cmp(path))
                .ok()
                .map(|index| &self.trait_headers[index])
                .or_else(|| self.held(path)?.trait_header(path))
        };
        found(path).or_else(|| found(&self.canonical(path)?))
    }

    /// The name a trait object of the crate trait `identity` has (D872):
    /// the first path, in order, of the headers the facades write for it,
    /// held ones too, so one trait reached through several crates' paths
    /// is one trait.
    #[cfg(feature = "rust-import")]
    pub(crate) fn trait_name(&self, identity: &[u8; 32]) -> Option<ImmutableString> {
        let held = self.held.iter().flat_map(|(held, _)| held.trait_headers());
        self.trait_headers
            .iter()
            .chain(held)
            .filter(|header| header.identity == *identity)
            .map(|header| header.path.to_string())
            .min()
            .map(ImmutableString::from)
    }

    /// The crates' impls of the trait whose identity is `identity` (D604).
    #[cfg(feature = "rust-import")]
    pub(crate) fn impl_headers(
        &self,
        identity: [u8; 32],
    ) -> impl Iterator<Item = &facade::ImplHeader> {
        self.owned_impl_headers(identity).map(|(header, _)| header)
    }

    /// The crates' impls of the trait whose identity is `identity`, each
    /// with the facade that declares it, whose types its header names: a
    /// game's `impl Framed<Message> for` its own opaque type names that
    /// type as only the game's facade has it.
    #[cfg(feature = "rust-import")]
    pub(crate) fn owned_impl_headers(
        &self,
        identity: [u8; 32],
    ) -> impl Iterator<Item = (&facade::ImplHeader, Option<&facade::Dependency>)> {
        // Each held facade's impls go where its install put them.
        let mut headers = Vec::new();
        let mut start = 0;
        for (held, position) in self.held_by_install() {
            self.registered_impls(identity, start..position, &mut headers);
            let owner = self.facade(&held.root().to_string());
            headers.extend(
                held.impl_headers(&identity)
                    .into_iter()
                    .map(|header| (header, owner)),
            );
            start = position;
        }
        self.registered_impls(identity, start..self.impl_headers.len(), &mut headers);
        headers.into_iter()
    }

    /// The registered impls of the trait `identity` within `range` of
    /// `impl_headers`, each with the facade that declares it, onto
    /// `headers`.
    #[cfg(feature = "rust-import")]
    fn registered_impls<'s>(
        &'s self,
        identity: [u8; 32],
        range: std::ops::Range<usize>,
        headers: &mut Vec<(&'s facade::ImplHeader, Option<&'s facade::Dependency>)>,
    ) {
        for (index, header) in self.impl_headers[range.clone()].iter().enumerate() {
            if header.trait_identity != identity {
                continue;
            }
            let index = range.start + index;
            let roots = self.impl_roots.partition_point(|(end, _)| *end <= index);
            let owner = self
                .impl_roots
                .get(roots)
                .and_then(|(_, root)| self.facade(root));
            headers.push((header, owner));
        }
    }

    /// The registered facade whose root is `root`.
    #[cfg(feature = "rust-import")]
    fn facade(&self, root: &str) -> Option<&facade::Dependency> {
        self.facades
            .binary_search_by(|facade| facade.root().cmp(root))
            .ok()
            .map(|index| &self.facades[index])
    }

    /// The held facades, each with where its impl headers go, in the order
    /// they were installed.
    #[cfg(feature = "rust-import")]
    fn held_by_install(&self) -> Vec<(&image::Held, usize)> {
        let mut held = self.held.iter().collect::<Vec<_>>();
        held.sort_by_key(|(_, installed)| *installed);
        held.into_iter()
            .map(|(facade, (position, _))| (facade, *position))
            .collect()
    }

    /// The builtin a facade names at `path` (`Entry::Builtin`).
    pub(crate) fn builtin_alias(&self, path: &path::Path) -> Option<&str> {
        let found = self.aliases.iter().find_map(|(alias, target)| {
            (alias == path && is_builtin_alias_target(target))
                .then(|| target.segments()[0].as_str())
        });
        #[cfg(feature = "rust-import")]
        let found = found.or_else(|| {
            let (_, target) = self
                .held(path)?
                .alias(path, |kind| kind == image::Kind::Builtin)?;
            Some(target.segments()[0].as_str())
        });
        found
    }

    /// The std path `qualified` names through a crate's re-export of a std
    /// item, as `iced::time::Duration` is `std::time::Duration`.
    pub(crate) fn std_alias(&self, qualified: &str) -> Option<ImmutableString> {
        let path = path::Path::parse(qualified).ok()?;
        let found = self
            .aliases
            .iter()
            .filter(|(_, target)| is_std_alias_target(target))
            .find_map(|(alias, target)| path.rebase(alias, target));
        #[cfg(feature = "rust-import")]
        let found = found.or_else(|| {
            self.held(&path)?
                .through_alias(&path, |kind| kind == image::Kind::Std)
        });
        found.map(|path| path.to_string().into())
    }

    /// The canonical spelling of a path through a facade re-export:
    /// `iced::event::Event::Keyboard` is `iced::Event::Keyboard`.
    pub(crate) fn canonical(&self, path: &path::Path) -> Option<path::Path> {
        let through = self.through_reexports(path);
        let path = through.as_ref().unwrap_or(path);
        let found = self
            .aliases
            .iter()
            .filter(|(_, target)| !is_builtin_alias_target(target))
            .find_map(|(alias, target)| path.rebase(alias, target));
        #[cfg(feature = "rust-import")]
        let found = found.or_else(|| {
            self.held(path)?
                .through_alias(path, |kind| kind != image::Kind::Builtin)
        });
        found.or(through)
    }

    /// `path` below the module each re-exported module above it names, as
    /// `iced_renderer::wgpu::Engine` is `iced_wgpu::Engine` where a facade
    /// holds iced_wgpu's entries once (`facade::Builder::
    /// imported_reexport`), or `None` when no re-exported module is above
    /// it.
    fn through_reexports(&self, path: &path::Path) -> Option<path::Path> {
        through_reexports(&self.reexports, path)
    }

    /// Each other path that names what `path` names through a re-exported
    /// module above it, as `iced_renderer::wgpu::Engine` names
    /// `iced_wgpu::Engine`: the paths the facade would hold had it held a
    /// re-exported module's entries at each of its paths.
    fn reexported(&self, path: &path::Path) -> Vec<path::Path> {
        let mut found = Vec::new();
        let mut next = 0;
        let mut current = path.clone();
        loop {
            let segments = current.segments();
            for length in 1..=segments.len() {
                let module = &segments[..length];
                let start = self
                    .reexported
                    .partition_point(|(target, _)| target.segments() < module);
                for (target, alias) in self.reexported[start..]
                    .iter()
                    .take_while(|(target, _)| target.segments() == module)
                {
                    if let Some(other) = current.rebase(target, alias) {
                        found.push(other);
                    }
                }
            }
            let Some(other) = found.get(next) else {
                return found;
            };
            current = other.clone();
            next += 1;
        }
    }

    /// [`Self::canonical`] of a written path, or the path itself.
    pub(crate) fn canonical_str(&self, qualified: &str) -> ImmutableString {
        path::Path::parse(qualified)
            .ok()
            .and_then(|path| self.canonical(&path))
            .map_or_else(|| qualified.into(), |path| path.to_string().into())
    }

    fn item_slice(&self, path: &path::Path) -> Option<&[item::Definition]> {
        let found = item_group(&self.items, path);
        #[cfg(feature = "rust-import")]
        let found = found.or_else(|| self.held(path)?.items(path));
        found
    }

    fn owner_for_path(&self, path: &path::Path) -> Option<&facade::Dependency> {
        let exact_facade_entry = self.has_module(path)
            // A module below a re-exported one is the one below the module
            // it names.
            || self
                .through_reexports(path)
                .is_some_and(|through| self.has_module(&through))
            || self
                .type_by_path(path)
                .is_some_and(type_def::Definition::exact_arity)
            || self.item_slice(path).is_some()
            || self.type_alias(path).is_some()
            || self
                .canonical(path)
                .is_some_and(|canonical| self.item_slice(&canonical).is_some());
        #[cfg(feature = "rust-import")]
        let exact_facade_entry =
            exact_facade_entry || self.pending_family(&path.to_string()).is_some();
        if !exact_facade_entry {
            return None;
        }
        let root = path.segments().first()?;
        self.facades
            .binary_search_by(|facade| facade.root().cmp(root))
            .ok()
            .map(|index| &self.facades[index])
    }

    fn facade_fingerprint_path(&self, root: &path::Path) -> Option<&facade::Fingerprint> {
        self.facades
            .binary_search_by(|facade| facade.root().cmp(root.name()))
            .ok()
            .map(|index| self.facades[index].fingerprint())
    }

    /// The first registered type, in path order, with `identity`. An
    /// imported identity is looked up in the imported index; scanning every
    /// type for each incoming one was quadratic in a facade's types.
    fn type_with_identity(&self, identity: &type_def::Identity) -> Option<&type_def::Definition> {
        #[cfg(feature = "rust-import")]
        if let Some(imported) = identity.imported() {
            let first = self
                .imported
                .partition_point(|entry| entry.identity < *imported);
            let eager = self.imported[first..]
                .iter()
                .take_while(|entry| entry.identity == *imported)
                .map(|entry| self.imported_aliases[entry.aliases.start])
                .min()
                .map(|index| &self.types[index]);
            // The first in path order, among the held facades' too.
            return eager
                .into_iter()
                .chain(self.held.iter().filter_map(|(held, _)| {
                    let first = held.identity_types(imported).into_iter().min()?;
                    Some(held.type_at(first))
                }))
                .min_by(|left, right| left.path_ref().cmp(right.path_ref()));
        }
        self.types
            .iter()
            .find(|existing| existing.identity() == identity)
    }

    /// Index the imported types an install placed at `types` (the
    /// merge's places, `merge::merge`) and its facade at `facade`: each
    /// type already indexed keeps its run, at the indices the install
    /// moved it and its facade to, and the new facade's types make runs
    /// of their own, as [`Self::rebuild_imported_index`] would order them.
    /// Rebuilding the whole index sorted every imported type again at each
    /// of a thousand installs (#1755).
    #[cfg(feature = "rust-import")]
    fn index_installed(&mut self, types: &[usize], facade: usize) {
        let mut keyed = Vec::new();
        for (order, place) in types.iter().enumerate() {
            let index = place + order;
            let definition = &self.types[index];
            let Some(identity) = definition.identity().imported() else {
                continue;
            };
            let path = definition.path_ref();
            let root = path.segments().first();
            // A type below another facade's root joins that facade's runs,
            // which only a whole rebuild orders.
            if root.is_none_or(|root| self.facades[facade].root() != root.as_str()) {
                self.rebuild_imported_index();
                return;
            }
            let hidden = path
                .segments()
                .iter()
                .any(|segment| path::is_hidden(segment));
            keyed.push((*identity, hidden, index));
        }
        for alias in &mut self.imported_aliases {
            *alias = merge::moved(types, *alias);
        }
        for entry in &mut self.imported {
            if entry.facade >= facade {
                entry.facade += 1;
            }
        }
        keyed.sort_unstable();
        let mut runs = Vec::<ImportedIndex>::new();
        for (identity, _, index) in keyed {
            let alias = self.imported_aliases.len();
            self.imported_aliases.push(index);
            match runs.last_mut() {
                Some(last) if last.identity == identity => last.aliases.end = alias + 1,
                _ => runs.push(ImportedIndex {
                    identity,
                    facade,
                    aliases: alias..alias + 1,
                }),
            }
        }
        merge::merge(&mut self.imported, runs, |left, right| {
            (left.identity, left.facade).cmp(&(right.identity, right.facade))
        });
    }

    #[cfg(feature = "rust-import")]
    fn rebuild_imported_index(&mut self) {
        // One sort of every imported type's (identity, facade, type index)
        // gives one run per identity and facade, its aliases in type order.
        // Inserting each type in place shifted every later range: quadratic
        // in the types, and run at each facade's install.
        // A path through a crate's hidden segment goes after the type's
        // public ones, so a type's first alias, the one its signatures
        // name it by, is the path rustc gives it whichever macros some
        // program reached the crate through (#893).
        let mut keyed = self
            .types
            .iter()
            .enumerate()
            .filter_map(|(type_index, definition)| {
                let identity = definition.identity().imported()?;
                let path = definition.path_ref();
                let root = path.segments().first()?;
                let facade = self
                    .facades
                    .binary_search_by(|candidate| candidate.root().cmp(root))
                    .expect("imported types retain their owner root");
                let hidden = path
                    .segments()
                    .iter()
                    .any(|segment| path::is_hidden(segment));
                Some((identity, facade, hidden, type_index))
            })
            .collect::<Vec<_>>();
        keyed.sort_unstable();
        self.imported.clear();
        self.imported_aliases.clear();
        for (identity, facade, _, type_index) in keyed {
            let alias = self.imported_aliases.len();
            self.imported_aliases.push(type_index);
            match self.imported.last_mut() {
                Some(last) if last.identity == *identity && last.facade == facade => {
                    last.aliases.end = alias + 1;
                }
                _ => self.imported.push(ImportedIndex {
                    identity: *identity,
                    facade,
                    aliases: alias..alias + 1,
                }),
            }
        }
    }
}

/// `path` below the module each of `reexports` above it names
/// ([`Registry::canonical`]), or `None` when none is above it. The module a
/// re-export names lies below none (`facade::Plan` checks it), so each
/// step leaves fewer segments to rebase and the walk ends.
pub(crate) fn through_reexports(
    reexports: &[(path::Path, path::Path)],
    path: &path::Path,
) -> Option<path::Path> {
    let mut through: Option<path::Path> = None;
    loop {
        let current = through.as_ref().unwrap_or(path);
        let segments = current.segments();
        let found = (1..=segments.len()).rev().find_map(|length| {
            reexports
                .binary_search_by(|(alias, _)| alias.segments().cmp(&segments[..length]))
                .ok()
        });
        let Some(index) = found else {
            return through;
        };
        let (alias, module) = &reexports[index];
        through = current.rebase(alias, module);
    }
}

fn item_group<'a>(
    items: &'a [item::Definition],
    path: &path::Path,
) -> Option<&'a [item::Definition]> {
    let start = items.partition_point(|item| item.path_ref() < path);
    let end = items.partition_point(|item| item.path_ref() <= path);
    (start != end).then_some(&items[start..end])
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "rust-import")]
    use super::facade;
    use super::{type_def, Error, Registry};

    #[test]
    fn registration_accepts_qualified_type_paths() {
        let mut registry = Registry::new();
        registry
            .register_type::<Column>("iced::widget::Column")
            .unwrap();

        assert_eq!(
            registry
                .type_("iced::widget::Column")
                .and_then(type_def::Definition::native_identity),
            Some(std::any::TypeId::of::<Column>())
        );
    }

    #[test]
    fn registration_rejects_invalid_type_paths() {
        let mut registry = Registry::new();

        assert!(matches!(
            registry.register_type::<Column>("iced::::Column"),
            Err(Error::InvalidTypePath(path)) if path == "iced::::Column"
        ));
    }

    #[cfg(feature = "rust-import")]
    #[test]
    fn registry_clone_matches_independent_exact_and_one_short_oracle() {
        fn path_bytes(path: &str) -> usize {
            std::mem::size_of::<super::path::Path>()
                + path.split("::").count() * std::mem::size_of::<crate::ImmutableString>()
                + path.split("::").map(str::len).sum::<usize>()
        }

        let mut builder = facade::Builder::new("oracle", facade::Abi::new(1).unwrap()).unwrap();
        builder.module("nested").unwrap();
        let definition = builder.finish().unwrap();
        let mut registry = Registry::new();
        registry.install(definition).unwrap();

        let fingerprint_bytes = std::mem::size_of::<facade::Fingerprint>()
            + facade::Fingerprint::COLLECTION_OVERHEAD
            + "oracle".len()
            + std::mem::size_of::<crate::ImmutableString>()
            + "oracle::nested".len();
        let dependency_bytes =
            std::mem::size_of::<facade::Dependency>() + "oracle".len() + fingerprint_bytes
                - std::mem::size_of::<facade::Fingerprint>();
        let oracle_items = 1 + 2 + 1;
        let oracle_bytes = std::mem::size_of::<Registry>()
            + path_bytes("oracle")
            + path_bytes("oracle::nested")
            + dependency_bytes;

        let mut exact = super::TestImportAllocation::new(oracle_items, oracle_bytes);
        registry.clone_for_import(&mut exact).unwrap();
        assert_eq!(exact.usage(), (oracle_items, oracle_bytes));

        let mut item_short = super::TestImportAllocation::new(oracle_items - 1, usize::MAX);
        assert!(registry.clone_for_import(&mut item_short).is_err());
        assert_eq!(item_short.usage(), (0, 0));

        let mut byte_short = super::TestImportAllocation::new(usize::MAX, oracle_bytes - 1);
        assert!(registry.clone_for_import(&mut byte_short).is_err());
        assert_eq!(byte_short.usage(), (0, 0));
    }

    /// A facade installed among others keeps each imported type's paths
    /// in the runs a whole rebuild of the index gives them, and each type
    /// at its place among the others' (#1755).
    #[cfg(feature = "rust-import")]
    #[test]
    fn an_install_indexes_its_types_as_a_whole_rebuild_does() {
        const HIDDEN: &str = "__rawr_crate_things_00000000000000ab";
        let shared = type_def::import::Identity::new([0x41; 32]);
        let own = type_def::import::Identity::new([0x42; 32]);
        let mut registry = Registry::new();
        for root in ["middle", "alpha", "omega"] {
            let mut builder = facade::Builder::new(root, facade::Abi::new(1).unwrap()).unwrap();
            let mut allocation = super::TestImportAllocation::default();
            builder.imported_module(HIDDEN, &mut allocation).unwrap();
            for (path, identity) in [("Thing", shared), (HIDDEN, shared), ("Other", own)] {
                let path = if path == HIDDEN {
                    format!("{path}::Thing")
                } else {
                    path.to_owned()
                };
                builder
                    .imported_type(
                        &path,
                        identity,
                        &[],
                        Box::default(),
                        Box::default(),
                        type_def::Shape::Opaque.into(),
                        &mut allocation,
                    )
                    .unwrap();
            }
            registry.install(builder.finish().unwrap()).unwrap();
        }
        let lookups = |registry: &Registry| {
            registry
                .imported
                .iter()
                .map(|entry| {
                    let paths = registry.imported_aliases[entry.aliases.clone()]
                        .iter()
                        .map(|alias| registry.types[*alias].path_ref().to_string())
                        .collect::<Vec<_>>();
                    (
                        entry.identity,
                        registry.facades[entry.facade].root().to_owned(),
                        paths,
                    )
                })
                .collect::<Vec<_>>()
        };
        let installed = lookups(&registry);
        let mut rebuilt = registry.clone();
        rebuilt.rebuild_imported_index();
        assert_eq!(installed, lookups(&rebuilt));
        assert_eq!(installed.len(), 6, "{installed:?}");
        let paths = registry
            .types
            .iter()
            .map(|definition| definition.path_ref().to_string())
            .collect::<Vec<_>>();
        let mut sorted = paths.clone();
        sorted.sort();
        assert_eq!(paths, sorted);
    }

    #[derive(Clone)]
    struct Column;
}
