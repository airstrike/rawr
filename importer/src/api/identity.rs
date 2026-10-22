//! Canonical Cargo, compiler-unit, definition, and export identities.

use serde::{Deserialize, Serialize};

use super::Retained;

/// Cargo's exact source-qualified package identity.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Package {
    cargo_id: Box<str>,
    name: Box<str>,
    version: Box<str>,
    source: Box<str>,
}

/// One exact normal library compiler unit within a package.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Unit {
    authority: Authority,
    target: Box<str>,
    target_kind: Box<str>,
    crate_name: Box<str>,
    compilation: Compilation,
    features: Box<[Box<str>]>,
    stable_crate_id: Box<str>,
}

/// Closed compiler-unit ownership authority.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Authority {
    /// A Cargo package selected or reached through the resolved unit graph.
    Cargo(Package),
    /// A compiler-shipped crate identified by the pinned sysroot universe.
    Sysroot(Sysroot),
}

/// Exact identity of one compiler-shipped crate.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Sysroot {
    compiler_commit: Box<str>,
    selected_target: Box<str>,
    crate_name: Box<str>,
    stable_crate_id: Box<str>,
}

/// Whether the exact compiler invocation is a host or selected-target unit.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Compilation {
    /// The unit executes for the pinned host.
    Host,
    /// The unit executes for the selected target.
    Target,
}

/// A Rust namespace that participates in canonical name resolution.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Namespace {
    /// Module, type, trait, or type-alias namespace.
    Type,
    /// Function, constant, static, variant, or constructor namespace.
    Value,
    /// Macro namespace retained only as explicit non-importable evidence.
    Macro,
}

impl Namespace {
    /// The stable canonical namespace spelling used in diagnostics.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Type => "type",
            Self::Value => "value",
            Self::Macro => "macro",
        }
    }
}

/// A definition identity derived from rustc's stable definition-path hash.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Definition {
    unit: Unit,
    def_path_hash: Box<str>,
    namespace: Namespace,
}

/// Exact identity of a compiler-recorded re-export item, which has no Rust namespace.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Reexport {
    unit: Unit,
    def_path_hash: Box<str>,
}

/// One public path in the namespace of a selected Rawr-side root alias.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Export {
    root_alias: Box<str>,
    path: Box<[Box<str>]>,
    namespace: Namespace,
}

impl Package {
    pub(super) fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        retained.string(&self.cargo_id)?;
        retained.string(&self.name)?;
        retained.string(&self.version)?;
        retained.string(&self.source)
    }

    pub(crate) fn retained_bytes(&self) -> Option<usize> {
        self.cargo_id
            .len()
            .checked_add(self.name.len())?
            .checked_add(self.version.len())?
            .checked_add(self.source.len())
    }

    pub(crate) fn new(
        cargo_id: Box<str>,
        name: Box<str>,
        version: Box<str>,
        source: Box<str>,
    ) -> Self {
        Self {
            cargo_id,
            name,
            version,
            source,
        }
    }

    /// Cargo's complete source/version-qualified package ID.
    pub fn cargo_id(&self) -> &str {
        &self.cargo_id
    }

    /// The declared Cargo package name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The exact Cargo package version.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Cargo's canonical package source identity.
    pub fn source(&self) -> &str {
        &self.source
    }
}

impl Unit {
    pub(super) fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        self.authority.retain(retained)?;
        retained.string(&self.target)?;
        retained.string(&self.target_kind)?;
        retained.string(&self.crate_name)?;
        retained.slice(&self.features)?;
        for feature in &self.features {
            retained.string(feature)?;
        }
        retained.string(&self.stable_crate_id)
    }

    pub(crate) fn retained_bytes(&self) -> Option<usize> {
        let features = self.features.iter().try_fold(
            std::mem::size_of::<Box<str>>().checked_mul(self.features.len())?,
            |bytes, feature| bytes.checked_add(feature.len()),
        )?;
        self.authority
            .retained_bytes()?
            .checked_add(self.target.len())?
            .checked_add(self.target_kind.len())?
            .checked_add(self.crate_name.len())?
            .checked_add(features)?
            .checked_add(self.stable_crate_id.len())
    }

    pub(crate) fn new(
        authority: Authority,
        target: Box<str>,
        target_kind: Box<str>,
        crate_name: Box<str>,
        compilation: Compilation,
        features: Box<[Box<str>]>,
        stable_crate_id: Box<str>,
    ) -> Self {
        Self {
            authority,
            target,
            target_kind,
            crate_name,
            compilation,
            features,
            stable_crate_id,
        }
    }

    /// The closed Cargo-or-sysroot authority owning this compiler unit.
    pub const fn authority(&self) -> &Authority {
        &self.authority
    }

    /// Borrow Cargo package evidence when this unit is Cargo-owned.
    pub const fn package(&self) -> Option<&Package> {
        self.authority.package()
    }

    /// Cargo's target identity for the selected library unit.
    pub fn target(&self) -> &str {
        &self.target
    }

    /// Cargo's exact target kind.
    pub fn target_kind(&self) -> &str {
        &self.target_kind
    }

    /// The rustc crate name used by the invocation.
    pub fn crate_name(&self) -> &str {
        &self.crate_name
    }

    /// Whether this is a host or selected-target compilation.
    pub const fn compilation(&self) -> &Compilation {
        &self.compilation
    }

    /// Exact enabled Cargo feature set for this compiler unit.
    pub fn features(&self) -> impl ExactSizeIterator<Item = &str> {
        self.features.iter().map(AsRef::as_ref)
    }

    /// rustc's local StableCrateId for this exact compiler unit.
    pub fn stable_crate_id(&self) -> &str {
        &self.stable_crate_id
    }
}

impl Authority {
    fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        match self {
            Self::Cargo(package) => package.retain(retained),
            Self::Sysroot(sysroot) => sysroot.retain(retained),
        }
    }

    fn retained_bytes(&self) -> Option<usize> {
        match self {
            Self::Cargo(package) => package.retained_bytes(),
            Self::Sysroot(sysroot) => sysroot.retained_bytes(),
        }
    }

    /// Borrow Cargo package evidence when this is a Cargo-owned unit.
    pub const fn package(&self) -> Option<&Package> {
        match self {
            Self::Cargo(package) => Some(package),
            Self::Sysroot(_) => None,
        }
    }

    /// Borrow sysroot evidence when this is a compiler-owned unit.
    pub const fn sysroot(&self) -> Option<&Sysroot> {
        match self {
            Self::Cargo(_) => None,
            Self::Sysroot(sysroot) => Some(sysroot),
        }
    }
}

impl Sysroot {
    pub(super) fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        retained.string(&self.compiler_commit)?;
        retained.string(&self.selected_target)?;
        retained.string(&self.crate_name)?;
        retained.string(&self.stable_crate_id)
    }

    fn retained_bytes(&self) -> Option<usize> {
        self.compiler_commit
            .len()
            .checked_add(self.selected_target.len())?
            .checked_add(self.crate_name.len())?
            .checked_add(self.stable_crate_id.len())
    }

    pub(crate) fn new(
        compiler_commit: Box<str>,
        selected_target: Box<str>,
        crate_name: Box<str>,
        stable_crate_id: Box<str>,
    ) -> Self {
        Self {
            compiler_commit,
            selected_target,
            crate_name,
            stable_crate_id,
        }
    }

    /// The exact pinned compiler commit owning this crate universe.
    pub fn compiler_commit(&self) -> &str {
        &self.compiler_commit
    }

    /// The exact compilation target whose sysroot was loaded.
    pub fn selected_target(&self) -> &str {
        &self.selected_target
    }

    /// rustc's canonical crate name.
    pub fn crate_name(&self) -> &str {
        &self.crate_name
    }

    /// rustc's stable crate identity in lowercase canonical hex.
    pub fn stable_crate_id(&self) -> &str {
        &self.stable_crate_id
    }
}

impl Definition {
    pub(super) fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        self.unit.retain(retained)?;
        retained.string(&self.def_path_hash)
    }

    pub(crate) fn retained_bytes(&self) -> Option<usize> {
        self.unit
            .retained_bytes()?
            .checked_add(self.def_path_hash.len())
    }

    pub(crate) fn new(unit: Unit, def_path_hash: Box<str>, namespace: Namespace) -> Self {
        Self {
            unit,
            def_path_hash,
            namespace,
        }
    }

    /// The exact compiler unit owning the definition.
    pub const fn unit(&self) -> &Unit {
        &self.unit
    }

    /// rustc's stable definition-path hash, never a traversal index.
    pub fn def_path_hash(&self) -> &str {
        &self.def_path_hash
    }

    /// The definition's Rust namespace.
    pub const fn namespace(&self) -> Namespace {
        self.namespace
    }
}

impl Reexport {
    pub(super) fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        self.unit.retain(retained)?;
        retained.string(&self.def_path_hash)
    }

    pub(crate) fn new(unit: Unit, def_path_hash: Box<str>) -> Self {
        Self {
            unit,
            def_path_hash,
        }
    }

    /// The exact Cargo or sysroot compiler unit containing the use item.
    pub const fn unit(&self) -> &Unit {
        &self.unit
    }

    /// rustc's stable definition-path hash for the use item.
    pub fn def_path_hash(&self) -> &str {
        &self.def_path_hash
    }
}

impl Export {
    pub(super) fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        retained.string(&self.root_alias)?;
        retained.slice(&self.path)?;
        for segment in &self.path {
            retained.string(segment)?;
        }
        Some(())
    }

    pub(crate) fn retained_bytes(&self) -> Option<usize> {
        let path = self.path.iter().try_fold(
            std::mem::size_of::<Box<str>>().checked_mul(self.path.len())?,
            |bytes, segment| bytes.checked_add(segment.len()),
        )?;
        self.root_alias.len().checked_add(path)
    }

    pub(crate) fn new(root_alias: Box<str>, path: Box<[Box<str>]>, namespace: Namespace) -> Self {
        Self {
            root_alias,
            path,
            namespace,
        }
    }

    /// The Rawr-side alias that selected the reachable namespace.
    pub fn root_alias(&self) -> &str {
        &self.root_alias
    }

    /// The unchanged source-visible canonical path segments.
    pub fn path(&self) -> impl ExactSizeIterator<Item = &str> {
        self.path.iter().map(AsRef::as_ref)
    }

    /// The public path's Rust namespace.
    pub const fn namespace(&self) -> Namespace {
        self.namespace
    }

    pub(crate) fn into_parts(self) -> (Box<str>, Box<[Box<str>]>, Namespace) {
        (self.root_alias, self.path, self.namespace)
    }
}
