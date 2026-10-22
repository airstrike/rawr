//! Host metadata consulted while resolving Rawr source.

pub(crate) mod endpoint;
pub mod value;
mod error;
pub mod facade;
pub mod item;
pub mod operation;
pub(crate) mod path;
pub(crate) mod projection;
pub mod runtime;
pub mod signature;
pub mod stream;
pub(crate) mod structural;
pub mod support;
pub mod type_def;
pub mod type_ref;
pub mod usage;

pub use error::Error;

use crate::tokenizer::is_valid_identifier;
use crate::ImmutableString;
use std::any::TypeId;
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

const BUILTIN_TYPES: &[&str] = &[
    "bool", "char", "f64", "i64", "String", "i8", "i16", "i32", "isize", "u8", "u16", "u32", "u64",
    "usize", "f32",
];

/// The runtime builtin a Rust numeric type is: every integer kind an
/// `i64`, `f32` an `f64` (the checker keeps the kinds apart).
pub(crate) fn runtime_builtin(name: &str) -> &str {
    match name {
        "i8" | "i16" | "i32" | "isize" | "u8" | "u16" | "u32" | "u64" | "usize" => "i64",
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
    supports: Vec<support::SignatureContract>,
    endpoints: Vec<endpoint::Contract>,
    #[cfg(feature = "rust-import")]
    imported: Vec<ImportedIndex>,
    #[cfg(feature = "rust-import")]
    imported_aliases: Vec<usize>,
    #[cfg(feature = "rust-import")]
    structural: Vec<structural::Definition>,
    items: Vec<item::Definition>,
    facades: Vec<facade::Dependency>,
    /// Facade re-exports, sorted: a path and the canonical path it names.
    aliases: Vec<(path::Path, path::Path)>,
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
    Endpoint(&'a endpoint::Contract),
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

impl Registry {
    /// Create a registry containing Rawr's built-in header types.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            modules: Vec::new(),
            types: Vec::new(),
            supports: Vec::new(),
            endpoints: Vec::new(),
            #[cfg(feature = "rust-import")]
            imported: Vec::new(),
            #[cfg(feature = "rust-import")]
            imported_aliases: Vec::new(),
            #[cfg(feature = "rust-import")]
            structural: Vec::new(),
            items: Vec::new(),
            facades: Vec::new(),
            aliases: Vec::new(),
        }
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn clone_for_import<A: ImportAllocation + ?Sized>(
        &self,
        allocation: &mut A,
    ) -> Result<Self, rawr_import::surface::Error> {
        let items = Some(1_usize)
            .and_then(|items| items.checked_add(self.modules.len()))
            .and_then(|items| items.checked_add(self.types.len()))
            .and_then(|items| items.checked_add(self.supports.len()))
            .and_then(|items| items.checked_add(self.endpoints.len()))
            .and_then(|items| items.checked_add(self.imported.len()))
            .and_then(|items| items.checked_add(self.imported_aliases.len()))
            .and_then(|items| items.checked_add(self.structural.len()))
            .and_then(|items| items.checked_add(self.items.len()))
            .and_then(|items| items.checked_add(self.facades.len()));
        let mut bytes = Some(std::mem::size_of::<Self>());
        for path in &self.modules {
            bytes = bytes.and_then(|bytes| bytes.checked_add(path.retained_bytes()?));
        }
        for definition in &self.types {
            bytes = bytes.and_then(|bytes| bytes.checked_add(definition.retained_bytes()?));
        }
        for support in &self.supports {
            bytes = bytes.and_then(|bytes| bytes.checked_add(support.retained_bytes()?));
        }
        for endpoint in &self.endpoints {
            bytes = bytes.and_then(|bytes| bytes.checked_add(endpoint.retained_bytes()?));
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
            .supports
            .try_reserve_exact(self.supports.len())
            .map_err(|_| rawr_import::surface::Error::Allocation)?;
        for support in &self.supports {
            clone.supports.push(support.try_clone()?);
        }
        clone
            .endpoints
            .try_reserve_exact(self.endpoints.len())
            .map_err(|_| rawr_import::surface::Error::Allocation)?;
        for endpoint in &self.endpoints {
            clone.endpoints.push(endpoint.try_clone()?);
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
            clone.aliases.push((alias.try_clone()?, target.try_clone()?));
        }
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
                existing: self.entry_kind(&canonical).unwrap_or("facade entry"),
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
        let support_count = definition.support_map().len();
        let endpoint_count = definition.endpoint_map().len();
        let item_count = definition.items().len();
        #[cfg(feature = "rust-import")]
        let imported_count = definition
            .types()
            .filter(|ty| ty.identity().imported().is_some())
            .count();
        self.modules
            .try_reserve(module_count)
            .map_err(|_| Error::Allocation)?;
        self.types
            .try_reserve(type_count)
            .map_err(|_| Error::Allocation)?;
        self.supports
            .try_reserve(support_count)
            .map_err(|_| Error::Allocation)?;
        self.endpoints
            .try_reserve(endpoint_count)
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
        insert_sorted_unique(&mut self.modules, parts.root);
        for module in parts.modules {
            insert_sorted_unique(&mut self.modules, module);
        }
        for entry in parts.types {
            let index = self
                .types
                .binary_search_by(|definition| definition.path_ref().cmp(entry.path_ref()))
                .expect_err("facade preflight rejects occupied type paths");
            self.types.insert(index, entry);
        }
        for entry in parts.supports {
            let index = self
                .supports
                .binary_search_by(|support| support.path_ref().cmp(entry.path_ref()))
                .expect_err("facade preflight rejects occupied support paths");
            self.supports.insert(index, entry);
        }
        for entry in parts.endpoints {
            let index = self
                .endpoints
                .binary_search_by(|endpoint| endpoint.path_ref().cmp(entry.path_ref()))
                .expect_err("facade preflight rejects occupied endpoint paths");
            self.endpoints.insert(index, entry);
        }
        for item in parts.items {
            let index = self
                .items
                .partition_point(|existing| existing.path_ref() <= item.path_ref());
            self.items.insert(index, item);
        }
        self.aliases
            .try_reserve(parts.aliases.len())
            .map_err(|_| Error::Allocation)?;
        self.aliases.extend(parts.aliases);
        self.aliases.sort();
        let facade_index = self
            .facades
            .binary_search_by(|facade| facade.root().cmp(dependency.root()))
            .expect_err("facade preflight rejects occupied roots");
        self.facades.insert(facade_index, dependency);
        #[cfg(feature = "rust-import")]
        self.rebuild_imported_index();
        Ok(self)
    }

    /// Look up a registered type by its canonical qualified path.
    #[must_use]
    pub fn type_(&self, path: &str) -> Option<&type_def::Definition> {
        let path = path::Path::parse(path).ok()?;
        self.types
            .binary_search_by(|definition| definition.path_ref().cmp(&path))
            .ok()
            .map(|index| &self.types[index])
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn structural(
        &self,
        path: &str,
        selector: &rawr_import::surface::identity::Selector,
    ) -> Option<&structural::Definition> {
        let fingerprint = selector.fingerprint();
        self.structural
            .iter()
            .find(|definition| definition.path() == path && definition.selector() == &fingerprint)
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn admit_structural<A: ImportAllocation + ?Sized>(
        &mut self,
        definition: structural::Definition,
        allocation: &mut A,
    ) -> Result<(), rawr_import::surface::Error> {
        let key = (definition.owner(), definition.path(), definition.selector());
        let index = match self.structural.binary_search_by(|candidate| {
            (candidate.owner(), candidate.path(), candidate.selector()).cmp(&key)
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

    pub(crate) fn source_type(&self, path: &str) -> Option<&type_def::Definition> {
        self.type_(path)
            .filter(|definition| definition.visibility() == type_def::Visibility::Public)
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
        let exact_alias = exact.is_some() || aliases.len() == 1;
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
            self.imported_aliases[self.imported[imported].aliases.clone()]
                .iter()
                .map(|index| self.types[*index].path()),
        )
    }

    /// Look up a registered static item by its canonical qualified path.
    #[must_use]
    pub fn item(&self, path: &str) -> Option<&item::Definition> {
        let path = path::Path::parse(path).ok()?;
        let items = self.item_slice(&path)?;
        let [item] = items else {
            return None;
        };
        Some(item)
    }

    /// The paths of the free functions and macros named `name`, in any
    /// module: `iced::widget::center` for `center`.
    pub(crate) fn functions_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = String> + 'a {
        self.items
            .iter()
            .filter(move |item| {
                item.path_ref().name() == name
                    && matches!(
                        item.kind(),
                        item::Kind::Function | item::Kind::IntrinsicFunction
                    )
            })
            .map(item::Definition::path)
    }

    /// Items declared directly under `owner`, such as a type's methods.
    pub(crate) fn members<'a>(&'a self, owner: &str) -> impl Iterator<Item = &'a item::Definition> {
        let owner = path::Path::parse(owner).ok();
        self.items
            .iter()
            .filter(move |item| owner.is_some() && item.path_ref().parent() == owner)
    }

    pub(crate) fn overloads(&self, path: &str) -> Option<&[item::Definition]> {
        let path = path::Path::parse(path).ok()?;
        self.item_slice(&path)
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

    pub(crate) fn stream_plan_types(
        &self,
    ) -> impl Iterator<Item = (&type_def::Definition, &facade::Dependency)> {
        self.types.iter().filter_map(|definition| {
            (definition.role() == type_def::Role::StreamPlan)
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

    pub(crate) fn facade_dependency(&self, root: &str) -> Option<facade::Dependency> {
        let root = path::Path::parse(root).ok()?;
        self.facades
            .binary_search_by(|facade| facade.root().cmp(root.name()))
            .ok()
            .map(|index| self.facades[index].clone())
    }

    pub(crate) fn is_builtin(&self, path: &str) -> bool {
        BUILTIN_TYPES.contains(&path)
    }

    pub(crate) fn entry(&self, qualified: &str) -> Option<Entry<'_>> {
        let path = path::Path::parse(qualified).ok()?;
        if self.modules.binary_search(&path).is_ok() {
            Some(match self.item_slice(&path) {
                Some(items) => Entry::ModuleItem(items),
                None => Entry::Module,
            })
        } else if let Some(ty) = self.type_by_path(&path) {
            (ty.visibility() == type_def::Visibility::Public).then_some(Entry::Type(ty))
        } else {
            if let Some((endpoint, _)) = self.operation_endpoint(qualified) {
                return Some(Entry::Endpoint(endpoint));
            }
            if path
                .parent()
                .and_then(|parent| self.type_by_path(&parent))
                .is_some_and(|owner| owner.visibility() == type_def::Visibility::Private)
            {
                return None;
            }
            self.item_slice(&path).map(Entry::Item)
        }
    }

    /// The names a glob import of `module` binds: each module, type and item
    /// directly inside it that resolves as an import, sorted and distinct.
    pub(crate) fn children(&self, module: &str) -> Vec<ImmutableString> {
        let Ok(module) = path::Path::parse(module) else {
            return Vec::new();
        };
        let inside = |path: &path::Path| {
            path.segments().len() == module.segments().len() + 1
                && path.segments().starts_with(module.segments())
        };
        let mut names = self
            .modules
            .iter()
            .chain(self.types.iter().map(type_def::Definition::path_ref))
            .chain(self.items.iter().map(item::Definition::path_ref))
            .filter(|path| inside(path))
            .filter(|path| self.entry(&path.segments().join("::")).is_some())
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
                existing: self.entry_kind(root).unwrap_or("facade"),
                incoming: "facade",
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
            if let Some(existing) = self
                .types
                .iter()
                .find(|existing| existing.identity() == incoming.identity())
                && !existing.same_identity_contract(incoming)
            {
                return Err(Error::ConflictingHostIdentity {
                    existing: existing.path().into(),
                    incoming: incoming.path().into(),
                });
            }
        }
        for incoming in definition.support_map() {
            self.ensure_registry_vacant(incoming.path_ref(), "signature support")?;
            if self
                .supports
                .binary_search_by(|support| support.path_ref().cmp(incoming.path_ref()))
                .is_ok()
            {
                return Err(Error::RegistryConflict {
                    path: incoming.path().into(),
                    existing: "signature support",
                    incoming: "signature support",
                });
            }
        }
        for incoming in definition.endpoint_map() {
            if self
                .endpoints
                .binary_search_by(|endpoint| endpoint.path_ref().cmp(incoming.path_ref()))
                .is_ok()
            {
                return Err(Error::RegistryConflict {
                    path: incoming.path().into(),
                    existing: "operation endpoint",
                    incoming: "operation endpoint",
                });
            }
        }
        for (path, items) in definition.item_sets() {
            self.ensure_registry_vacant(path, "item")?;
            for item in items {
                for export in item.signature().external_types() {
                    self.validate_export(item.path(), export)?;
                }
                for export in item.signature().support_exports() {
                    self.validate_support_export(item.path(), export)?;
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

    fn validate_support_export(
        &self,
        source: String,
        export: &support::SupportExport,
    ) -> Result<(), Error> {
        let Some(installed_facade) = self.facade_fingerprint_path(export.facade_root()) else {
            return Err(Error::MissingFacadeDependency {
                source: source.into(),
                facade: export.facade_root().to_string().into(),
            });
        };
        let target = self.support_by_path(export.contract().path_ref());
        if installed_facade != export.facade() || target != Some(export.contract()) {
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
        if self.modules.binary_search(path).is_ok() {
            Some("module")
        } else if self
            .types
            .binary_search_by(|definition| definition.path_ref().cmp(path))
            .is_ok()
        {
            Some("type")
        } else if self
            .supports
            .binary_search_by(|support| support.path_ref().cmp(path))
            .is_ok()
        {
            Some("signature support")
        } else if item_group(&self.items, path).is_some() {
            Some("item")
        } else {
            None
        }
    }

    fn type_by_path(&self, path: &path::Path) -> Option<&type_def::Definition> {
        let found = |path: &path::Path| {
            self.types
                .binary_search_by(|definition| definition.path_ref().cmp(path))
                .ok()
                .map(|index| &self.types[index])
        };
        found(path).or_else(|| found(&self.canonical(path)?))
    }

    /// The canonical spelling of a path through a facade re-export:
    /// `iced::event::Event::Keyboard` is `iced::Event::Keyboard`.
    pub(crate) fn canonical(&self, path: &path::Path) -> Option<path::Path> {
        self.aliases
            .iter()
            .find_map(|(alias, target)| path.rebase(alias, target))
    }

    /// [`Self::canonical`] of a written path, or the path itself.
    pub(crate) fn canonical_str(&self, qualified: &str) -> ImmutableString {
        path::Path::parse(qualified)
            .ok()
            .and_then(|path| self.canonical(&path))
            .map_or_else(|| qualified.into(), |path| path.to_string().into())
    }

    pub(crate) fn support_by_path(&self, path: &path::Path) -> Option<&support::SignatureContract> {
        self.supports
            .binary_search_by(|support| support.path_ref().cmp(path))
            .ok()
            .map(|index| &self.supports[index])
    }

    pub(crate) fn support_type(
        &self,
        path: &str,
    ) -> Option<(&support::SignatureContract, &facade::Dependency)> {
        let path = path::Path::parse(path).ok()?;
        let contract = self.support_by_path(&path)?;
        let root = path.segments().first()?;
        let owner = self
            .facades
            .binary_search_by(|facade| facade.root().cmp(root))
            .ok()
            .map(|index| &self.facades[index])?;
        Some((contract, owner))
    }

    pub(crate) fn operation_endpoint(
        &self,
        path: &str,
    ) -> Option<(&endpoint::Contract, &facade::Dependency)> {
        let path = path::Path::parse(path).ok()?;
        let contract = self
            .endpoints
            .binary_search_by(|endpoint| endpoint.path_ref().cmp(&path))
            .ok()
            .map(|index| &self.endpoints[index])?;
        let root = path.segments().first()?;
        let owner = self
            .facades
            .binary_search_by(|facade| facade.root().cmp(root))
            .ok()
            .map(|index| &self.facades[index])?;
        Some((contract, owner))
    }

    fn item_slice(&self, path: &path::Path) -> Option<&[item::Definition]> {
        item_group(&self.items, path)
    }

    fn owner_for_path(&self, path: &path::Path) -> Option<&facade::Dependency> {
        let exact_facade_entry = self.modules.binary_search(path).is_ok()
            || self
                .type_by_path(path)
                .is_some_and(type_def::Definition::exact_arity)
            || self.item_slice(path).is_some();
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

    #[cfg(feature = "rust-import")]
    fn rebuild_imported_index(&mut self) {
        self.imported.clear();
        self.imported_aliases.clear();
        for (type_index, definition) in self.types.iter().enumerate() {
            let Some(identity) = definition.identity().imported().cloned() else {
                continue;
            };
            let Some(root) = definition.path_ref().segments().first() else {
                continue;
            };
            let facade = self
                .facades
                .binary_search_by(|candidate| candidate.root().cmp(root))
                .expect("imported facade types retain their owner root");
            let key = self.imported.binary_search_by(|candidate| {
                candidate
                    .identity
                    .cmp(&identity)
                    .then_with(|| candidate.facade.cmp(&facade))
            });
            match key {
                Ok(index) => {
                    let insertion = self.imported[index].aliases.end;
                    self.imported_aliases.insert(insertion, type_index);
                    self.imported[index].aliases.end += 1;
                    for following in &mut self.imported[index + 1..] {
                        following.aliases.start += 1;
                        following.aliases.end += 1;
                    }
                }
                Err(index) => {
                    let insertion = self
                        .imported
                        .get(index)
                        .map_or(self.imported_aliases.len(), |entry| entry.aliases.start);
                    self.imported_aliases.insert(insertion, type_index);
                    for following in &mut self.imported[index..] {
                        following.aliases.start += 1;
                        following.aliases.end += 1;
                    }
                    self.imported.insert(
                        index,
                        ImportedIndex {
                            identity,
                            facade,
                            aliases: insertion..insertion + 1,
                        },
                    );
                }
            }
        }
    }
}

fn insert_sorted_unique<T: Ord>(entries: &mut Vec<T>, entry: T) {
    let index = entries
        .binary_search(&entry)
        .expect_err("facade preflight rejects occupied registry paths");
    entries.insert(index, entry);
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

    #[derive(Clone)]
    struct Column;
}
