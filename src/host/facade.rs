//! Atomic static host-facade definitions.

use crate::{capability, host, ImmutableString, Shared};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};

/// A non-zero facade ABI version.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Abi(u32);

impl Abi {
    /// Validate a facade ABI version.
    pub const fn new(version: u32) -> Result<Self, host::Error> {
        if version == 0 {
            Err(host::Error::InvalidFacadeAbi(version))
        } else {
            Ok(Self(version))
        }
    }

    /// Return the numeric ABI version.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// One opaque, fully validated static facade contract.
#[derive(Clone, Debug)]
pub struct Definition {
    root: host::path::Path,
    abi: Abi,
    modules: Box<[host::path::Path]>,
    types: Box<[host::type_def::Definition]>,
    supports: Box<[host::support::SignatureContract]>,
    endpoints: Box<[host::endpoint::Contract]>,
    items: Box<[host::item::Definition]>,
    aliases: Box<[(host::path::Path, host::path::Path)]>,
    fingerprint: Fingerprint,
}

pub(crate) struct RegistryParts {
    pub(crate) root: host::path::Path,
    pub(crate) modules: Box<[host::path::Path]>,
    pub(crate) types: Box<[host::type_def::Definition]>,
    pub(crate) supports: Box<[host::support::SignatureContract]>,
    pub(crate) endpoints: Box<[host::endpoint::Contract]>,
    pub(crate) items: Box<[host::item::Definition]>,
    pub(crate) aliases: Box<[(host::path::Path, host::path::Path)]>,
    pub(crate) fingerprint: Fingerprint,
}

/// A deterministic exact proof of a complete static facade contract.
#[derive(Clone, Debug, Eq)]
pub struct Fingerprint {
    root: ImmutableString,
    abi: Abi,
    compatibility: Option<[u8; 32]>,
    modules: Shared<[ImmutableString]>,
    types: Shared<[TypeProof]>,
    supports: Shared<[host::support::SignatureContract]>,
    endpoints: Shared<[host::endpoint::Contract]>,
    items: Shared<[host::item::Fingerprint]>,
    /// SHA-256 of the contract slices, computed whenever they are set, so
    /// hashing a proof that names this facade does not rewalk it.
    content: [u8; 32],
}

struct ContentHasher(Sha256);

impl Hasher for ContentHasher {
    fn finish(&self) -> u64 {
        0
    }

    fn write(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
}

/// Opaque proof of the one profile-derived facade self-cycle edge.
///
/// This proof can normalize application-semantic hashing only. It is not a
/// facade fingerprint and cannot satisfy registry or runtime compatibility.
#[doc(hidden)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProfileCycle {
    root: ImmutableString,
    abi: Abi,
    exact: [u8; 32],
    semantic: [u8; 32],
    schema: u32,
}

/// Borrowed proof that one exact profile facade may finalize stage-local type
/// identities only for non-authoritative application semantics.
#[cfg(feature = "rust-import")]
pub(crate) struct ProfileFinalization<'a> {
    cycle: &'a ProfileCycle,
    exact: &'a Fingerprint,
    runtime_representations_are_valid: core::cell::Cell<bool>,
    /// Facades already normalized by this finalization (input, output,
    /// completed count). Normalization is pure in the input, so every type
    /// reference naming the same facade reuses one output and its content hash.
    normalized: core::cell::RefCell<Vec<(Fingerprint, Fingerprint, usize)>>,
    /// Facades the profile built beside its exact facade: they carry its exact
    /// seal under another root, so their own types finalize like its types.
    companions: core::cell::RefCell<Vec<Fingerprint>>,
}

/// Opaque proof that one installed declaration belongs to an exact facade.
///
/// Dependencies are attached by [`host::Registry`](crate::host::Registry)
/// only after a complete facade has finished validation. Legacy direct
/// registrations deliberately have no dependency proof.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Dependency {
    root: ImmutableString,
    fingerprint: Fingerprint,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct TypeProof {
    contract: host::type_def::Contract,
    projections: Box<[host::type_def::Projection]>,
}

/// Collects and validates one facade before it can enter a registry.
#[derive(Debug)]
pub struct Builder {
    root: host::path::Path,
    abi: Abi,
    compatibility: Option<[u8; 32]>,
    modules: Vec<host::path::Path>,
    types: Vec<host::type_def::Definition>,
    supports: Vec<host::support::SignatureContract>,
    endpoints: Vec<host::endpoint::Contract>,
    items: Vec<(host::path::Path, Vec<host::item::Definition>)>,
    #[cfg(feature = "rust-import")]
    profile_items: Vec<host::item::Definition>,
    projections: Vec<ProjectionDeclaration>,
    aliases: Vec<(host::path::Path, host::path::Path)>,
}

#[derive(Debug)]
struct ProjectionDeclaration {
    source: host::path::Path,
    target: ProjectionTarget,
    arguments: Box<[host::type_def::Argument]>,
}

// Boxing `External` would add an infallible allocation to a builder that
// reserves its storage fallibly.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
enum ProjectionTarget {
    Local(host::path::Path),
    External(host::type_def::Export),
}

impl Builder {
    /// Begin a facade rooted at one script-visible namespace.
    pub fn new(root: impl Into<ImmutableString>, abi: Abi) -> Result<Self, host::Error> {
        let root = host::path::Path::parse(root)?;
        if root.segments().len() != 1 {
            return Err(host::Error::InvalidFacadeRoot(root.to_string().into()));
        }
        Ok(Self {
            root,
            abi,
            compatibility: None,
            modules: Vec::new(),
            types: Vec::new(),
            supports: Vec::new(),
            endpoints: Vec::new(),
            items: Vec::new(),
            #[cfg(feature = "rust-import")]
            profile_items: Vec::new(),
            projections: Vec::new(),
            aliases: Vec::new(),
        })
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn new_imported<A: host::ImportAllocation + ?Sized>(
        root: &str,
        abi: Abi,
        allocation: &mut A,
    ) -> Result<Self, host::Error> {
        let root = host::path::Path::parse_imported(root, allocation)?;
        if root.segments().len() != 1 {
            return Err(host::Error::InvalidFacadeRoot(root.to_string().into()));
        }
        Ok(Self {
            root,
            abi,
            compatibility: None,
            modules: Vec::new(),
            types: Vec::new(),
            supports: Vec::new(),
            endpoints: Vec::new(),
            items: Vec::new(),
            profile_items: Vec::new(),
            projections: Vec::new(),
            aliases: Vec::new(),
        })
    }

    /// Seal a facade with its profile's exact compatibility seal. A profile seals
    /// its exact facade and any companion facade it builds beside it.
    #[cfg(feature = "rust-import")]
    #[doc(hidden)]
    pub fn compatibility_seal(&mut self, seal: [u8; 32]) -> Result<&mut Self, host::Error> {
        if self.compatibility.replace(seal).is_some() {
            return Err(host::Error::DuplicateFacadeEntry {
                path: self.root.to_string().into(),
                existing: "compatibility seed",
                incoming: "compatibility seed",
            });
        }
        Ok(self)
    }

    /// Carry one of the compatibility seed's re-exports into an imported
    /// facade. Both paths are already qualified. The seed's own type stays the
    /// meaning of the re-export, so an iced path the facade declares as an
    /// alias resolves even when the importer rejects the Rust item there.
    #[cfg(feature = "rust-import")]
    pub(crate) fn compatibility_alias(
        &mut self,
        path: &host::path::Path,
        target: &host::path::Path,
    ) -> Result<&mut Self, host::Error> {
        self.ensure_vacant(path, "re-export")?;
        if self.aliases.iter().any(|(alias, _)| alias == path) {
            return Err(host::Error::DuplicateFacadeEntry {
                path: path.to_string().into(),
                existing: "re-export",
                incoming: "re-export",
            });
        }
        self.aliases
            .try_reserve(1)
            .map_err(|_| host::Error::Allocation)?;
        self.aliases.push((path.clone(), target.clone()));
        Ok(self)
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn imported_module<A: host::ImportAllocation + ?Sized>(
        &mut self,
        path: &str,
        allocation: &mut A,
    ) -> Result<&mut Self, host::Error> {
        let relative = host::path::Path::parse_imported(path, allocation)?;
        let path = host::path::Path::qualify_imported(&self.root, relative, allocation)?;
        let existing = if self.modules.binary_search(&path).is_ok() {
            Some("module")
        } else if self.type_index(&path).is_ok() {
            Some("type")
        } else if self.support_index(&path).is_ok() {
            Some("signature support")
        } else if self.aliases.iter().any(|(alias, _)| *alias == path) {
            // A re-export such as iced's `widget::Component` names its
            // type; the trait of that name stays the facade's.
            Some("re-export")
        } else {
            None
        };
        if let Some(existing) = existing {
            return Err(host::Error::DuplicateFacadeEntry {
                path: path.to_string().into(),
                existing,
                incoming: "module",
            });
        }
        let index = self
            .modules
            .binary_search(&path)
            .expect_err("duplicate modules were rejected above");
        allocation
            .retain_checked(Some(1), Some(std::mem::size_of::<host::path::Path>()))
            .map_err(|_| host::Error::Allocation)?;
        self.modules
            .try_reserve(1)
            .map_err(|_| host::Error::Allocation)?;
        self.modules.insert(index, path);
        Ok(self)
    }

    /// Declare a relative module path.
    pub fn module(&mut self, path: impl Into<ImmutableString>) -> Result<&mut Self, host::Error> {
        let relative = host::path::Path::parse(path)?;
        let path = host::path::Path::qualify(&self.root, &relative);
        let existing = if self.modules.binary_search(&path).is_ok() {
            Some("module")
        } else if self.type_index(&path).is_ok() {
            Some("type")
        } else {
            None
        };
        if let Some(existing) = existing {
            return Err(host::Error::DuplicateFacadeEntry {
                path: path.to_string().into(),
                existing,
                incoming: "module",
            });
        }
        let index = self
            .modules
            .binary_search(&path)
            .expect_err("duplicate modules were rejected above");
        self.modules
            .try_reserve(1)
            .map_err(|_| host::Error::Allocation)?;
        self.modules.insert(index, path);
        Ok(self)
    }

    /// Re-export this facade's type `target` at `path`, as Rust's `pub use`:
    /// iced's `iced::Event` is also `iced::event::Event`. Both are relative.
    pub fn alias(
        &mut self,
        path: impl Into<ImmutableString>,
        target: impl Into<ImmutableString>,
    ) -> Result<&mut Self, host::Error> {
        let path = host::path::Path::qualify(&self.root, &host::path::Path::parse(path)?);
        let target = host::path::Path::qualify(&self.root, &host::path::Path::parse(target)?);
        self.ensure_vacant(&path, "re-export")?;
        if self.aliases.iter().any(|(alias, _)| *alias == path) {
            return Err(host::Error::DuplicateFacadeEntry {
                path: path.to_string().into(),
                existing: "re-export",
                incoming: "re-export",
            });
        }
        self.aliases
            .try_reserve(1)
            .map_err(|_| host::Error::Allocation)?;
        self.aliases.push((path, target));
        Ok(self)
    }

    /// Re-export this facade's enum variant `variant` at `path`, as Rust's
    /// `pub use Alignment::Center;`: `variant` is the variant's path through
    /// its enum (`alignment::Alignment::Center`). Both are relative.
    #[cfg(feature = "rust-import")]
    pub fn variant_alias(
        &mut self,
        path: impl Into<ImmutableString>,
        variant: &str,
    ) -> Result<&mut Self, host::Error> {
        let path = host::path::Path::qualify(&self.root, &host::path::Path::parse(path)?);
        let target = host::path::Path::qualify(&self.root, &host::path::Path::parse(variant)?);
        self.ensure_vacant(&path, "re-export")?;
        if self.aliases.iter().any(|(alias, _)| *alias == path) {
            return Err(host::Error::DuplicateFacadeEntry {
                path: path.to_string().into(),
                existing: "re-export",
                incoming: "re-export",
            });
        }
        self.aliases
            .try_reserve(1)
            .map_err(|_| host::Error::Allocation)?;
        self.aliases.push((path, target));
        Ok(self)
    }

    /// Name the builtin type `builtin` at `path`, as Rust's `pub type Uri =
    /// String;` in iced's `markdown` names `String`. `path` is relative.
    #[cfg(feature = "rust-import")]
    pub fn builtin_alias(
        &mut self,
        path: impl Into<ImmutableString>,
        builtin: &str,
    ) -> Result<&mut Self, host::Error> {
        let path = host::path::Path::qualify(&self.root, &host::path::Path::parse(path)?);
        let target = host::path::Path::parse(builtin)?;
        if !host::is_builtin_alias_target(&target) {
            return Err(host::Error::DuplicateFacadeEntry {
                path: path.to_string().into(),
                existing: "re-export of an unknown builtin",
                incoming: "re-export",
            });
        }
        self.ensure_vacant(&path, "re-export")?;
        if self.aliases.iter().any(|(alias, _)| *alias == path) {
            return Err(host::Error::DuplicateFacadeEntry {
                path: path.to_string().into(),
                existing: "re-export",
                incoming: "re-export",
            });
        }
        self.aliases
            .try_reserve(1)
            .map_err(|_| host::Error::Allocation)?;
        self.aliases.push((path, target));
        Ok(self)
    }

    /// Declare a host type with explicit structural parameters and capabilities.
    pub fn type_<T: 'static>(
        &mut self,
        path: impl Into<ImmutableString>,
        parameters: &[&str],
        capabilities: &[capability::Kind],
    ) -> Result<&mut Self, host::Error> {
        let relative = host::path::Path::parse(path)?;
        let path = host::path::Path::qualify(&self.root, &relative);
        self.ensure_vacant(&path, "type")?;
        let definition = host::type_def::Definition::declared::<T>(
            path,
            parameters,
            capabilities,
            host::type_def::Storage::Owned,
            host::type_def::Role::Ordinary,
            host::usage::Policy::Ordinary,
            host::type_def::Visibility::Public,
        )?;
        self.insert_type(definition)?;
        Ok(self)
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn imported_type(
        &mut self,
        path: &str,
        identity: host::type_def::import::Identity,
        implemented: &[crate::capability::Kind],
        parameters: Box<[ImmutableString]>,
        defaults: Box<[Option<host::type_ref::Type>]>,
        shape: crate::Shared<host::type_def::Shape>,
        allocation: &mut impl host::ImportAllocation,
    ) -> Result<&mut Self, host::Error> {
        let relative = host::path::Path::parse_imported(path, allocation)?;
        let path = host::path::Path::qualify_imported(&self.root, relative, allocation)?;
        self.ensure_vacant(&path, "type")?;
        let definition = host::type_def::Definition::imported(
            path,
            identity,
            implemented,
            parameters,
            defaults,
            shape,
        )?;
        self.insert_type(definition)?;
        Ok(self)
    }

    /// Retain one linked profile type in an otherwise imported facade.
    #[cfg(feature = "rust-import")]
    #[doc(hidden)]
    pub fn profile_type(
        &mut self,
        definition: &host::type_def::Definition,
    ) -> Result<&mut Self, host::Error> {
        let path = definition.path_ref();
        if path.segments().first() != self.root.segments().first() {
            return Err(host::Error::InvalidFacadeRoot(path.to_string().into()));
        }
        self.ensure_vacant(path, "type")?;
        self.insert_type(definition.clone())?;
        Ok(self)
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn compatibility_type<A: host::ImportAllocation + ?Sized>(
        &mut self,
        definition: &host::type_def::Definition,
        allocation: &mut A,
    ) -> Result<&mut Self, host::Error> {
        let path = definition.path_ref();
        if path.segments().first() != self.root.segments().first() {
            return Err(host::Error::InvalidFacadeRoot(path.to_string().into()));
        }
        if let Ok(index) = self.type_index(path) {
            let existing = &self.types[index];
            if existing.contract() != definition.contract() || !existing.projections().is_empty() {
                return Err(host::Error::ConflictingHostIdentity {
                    existing: existing.path().into(),
                    incoming: definition.path().into(),
                });
            }
            return Ok(self);
        }
        self.ensure_vacant(path, "compatibility type")?;
        allocation
            .retain_checked(Some(1), definition.retained_bytes())
            .map_err(|_| host::Error::Allocation)?;
        let definition = definition
            .try_clone_without_projections()
            .map_err(|_| host::Error::Allocation)?;
        self.insert_type(definition)?;
        Ok(self)
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn install_compatibility_projections(
        &mut self,
        seed: &Definition,
    ) -> Result<(), host::Error> {
        let mut required_rows = 0_usize;
        for source in seed
            .types()
            .filter(|definition| !definition.projections().is_empty())
        {
            let final_source = self
                .type_index(source.path_ref())
                .ok()
                .map(|index| &self.types[index])
                .ok_or_else(|| host::Error::UnknownProjectionSource {
                    source: source.path().into(),
                })?;
            if final_source.contract() != source.contract()
                || !final_source.projections().is_empty()
            {
                return Err(host::Error::ConflictingHostIdentity {
                    existing: final_source.path().into(),
                    incoming: source.path().into(),
                });
            }
            for required in source.projections() {
                required_rows = required_rows
                    .checked_add(1)
                    .ok_or(host::Error::Allocation)?;
                let target_path = required.target_contract().path_ref();
                let final_target = self
                    .type_index(target_path)
                    .ok()
                    .map(|index| &self.types[index])
                    .ok_or_else(|| host::Error::UnknownFacadeType {
                        item: source.path().into(),
                        ty: required.target().into(),
                    })?;
                if final_target.contract() != *required.target_contract() {
                    return Err(host::Error::ConflictingHostIdentity {
                        existing: final_target.path().into(),
                        incoming: required.target().into(),
                    });
                }
                let matches = self
                    .projections
                    .iter()
                    .filter(|projection| {
                        projection.source == *source.path_ref()
                            && matches!(
                                &projection.target,
                                ProjectionTarget::Local(target) if target == target_path
                            )
                            && projection.arguments.as_ref() == required.arguments()
                    })
                    .count();
                if matches > 1 {
                    return Err(host::Error::UnknownProjectionSource {
                        source: source.path().into(),
                    });
                }
                if matches == 0 {
                    let prefix = format!("{}::", self.root);
                    let source_path = source.path();
                    let target_path = required.target();
                    let source_path = source_path.strip_prefix(&prefix).ok_or_else(|| {
                        host::Error::InvalidFacadeRoot(source_path.clone().into())
                    })?;
                    let target_path = target_path.strip_prefix(&prefix).ok_or_else(|| {
                        host::Error::InvalidFacadeRoot(target_path.clone().into())
                    })?;
                    self.project_with(source_path, target_path, required.arguments())?;
                }
            }
        }
        let installed_rows = self
            .projections
            .iter()
            .filter(|projection| {
                seed.types().any(|source| {
                    source.path_ref() == &projection.source && !source.projections().is_empty()
                })
            })
            .count();
        if installed_rows != required_rows {
            return Err(host::Error::UnknownProjectionSource {
                source: seed.root().into(),
            });
        }
        Ok(())
    }

    /// Seal one compiler/profile-authenticated signature-only linked type.
    #[cfg(feature = "rust-import")]
    #[doc(hidden)]
    #[allow(
        clippy::too_many_arguments,
        reason = "the signature-support proof bundle is intentionally indivisible"
    )]
    pub fn profile_support_type(
        &mut self,
        definition: &host::type_def::Definition,
        role: &str,
        site: &rawr_import::surface::facade::AbiSite,
        profile: [u8; 32],
        linked: [u8; 32],
    ) -> Result<host::support::Local, host::Error> {
        self.validate_profile_support_type(definition, site)?;
        self.insert_profile_support_type(
            definition,
            role,
            *site.selector(),
            profile,
            linked,
            Some(site.definition_identity()),
        )
    }

    /// Install one exact profile-semantic ordinary type beside its independently
    /// sealed signature-support contract.
    #[cfg(feature = "rust-import")]
    #[doc(hidden)]
    #[allow(
        clippy::too_many_arguments,
        reason = "the semantic/support proof join is intentionally indivisible"
    )]
    pub fn profile_semantic_support_type(
        &mut self,
        definition: &host::type_def::Definition,
        role: &str,
        site: &rawr_import::surface::facade::AbiSite,
        profile: [u8; 32],
        linked: [u8; 32],
    ) -> Result<host::support::Local, host::Error> {
        self.validate_profile_support_type(definition, site)?;
        self.insert_profile_semantic_support_type(
            definition,
            role,
            *site.selector(),
            profile,
            linked,
        )
    }

    #[cfg(feature = "rust-import")]
    fn validate_profile_support_type(
        &self,
        definition: &host::type_def::Definition,
        site: &rawr_import::surface::facade::AbiSite,
    ) -> Result<(), host::Error> {
        let path = definition.path_ref();
        let relative = host::path::Path::parse(site.path())?;
        let expected = host::path::Path::qualify(&self.root, &relative);
        if site.namespace() != rawr_import::surface::facade::Namespace::Type
            || site.root() != self.root.segments()[0].as_str()
            || path != &expected
            || definition.visibility() != host::type_def::Visibility::Public
            || !definition.parameters().is_empty()
        {
            return Err(host::Error::InvalidFacadeRoot(path.to_string().into()));
        }
        Ok(())
    }

    #[cfg(all(test, feature = "rust-import"))]
    pub(crate) fn fixture_profile_support_type(
        &mut self,
        definition: &host::type_def::Definition,
        role: &str,
        site: [u8; 32],
        profile: [u8; 32],
        linked: [u8; 32],
    ) -> Result<host::support::Local, host::Error> {
        self.insert_profile_support_type(definition, role, site, profile, linked, None)
    }

    #[cfg(all(test, feature = "rust-import"))]
    pub(crate) fn fixture_profile_semantic_support_type(
        &mut self,
        definition: &host::type_def::Definition,
        role: &str,
        site: [u8; 32],
        profile: [u8; 32],
        linked: [u8; 32],
    ) -> Result<host::support::Local, host::Error> {
        self.insert_profile_semantic_support_type(definition, role, site, profile, linked)
    }

    #[cfg(feature = "rust-import")]
    fn insert_profile_semantic_support_type(
        &mut self,
        definition: &host::type_def::Definition,
        role: &str,
        site: [u8; 32],
        profile: [u8; 32],
        linked: [u8; 32],
    ) -> Result<host::support::Local, host::Error> {
        let path = definition.path_ref();
        self.ensure_vacant(path, "profile semantic type")?;
        let representation =
            definition
                .native_identity()
                .ok_or_else(|| host::Error::ConflictingHostIdentity {
                    existing: path.to_string().into(),
                    incoming: path.to_string().into(),
                })?;
        let contract = host::support::SignatureContract::new(
            path.clone(),
            role,
            site,
            profile,
            linked,
            representation,
        )?;
        let type_index = self
            .type_index(path)
            .expect_err("semantic type vacancy is checked before insertion");
        let support_index = self
            .support_index(path)
            .expect_err("support vacancy is checked before insertion");
        self.types
            .try_reserve_exact(1)
            .map_err(|_| host::Error::Allocation)?;
        self.supports
            .try_reserve_exact(1)
            .map_err(|_| host::Error::Allocation)?;
        self.types.insert(type_index, definition.clone());
        self.supports.insert(support_index, contract.clone());
        Ok(host::support::Local::new(contract))
    }

    #[cfg(feature = "rust-import")]
    fn insert_profile_support_type(
        &mut self,
        definition: &host::type_def::Definition,
        role: &str,
        site: [u8; 32],
        profile: [u8; 32],
        linked: [u8; 32],
        ordinary_identity: Option<&[u8; 32]>,
    ) -> Result<host::support::Local, host::Error> {
        let path = definition.path_ref();
        let existing = if self.modules.binary_search(path).is_ok() {
            Some("module")
        } else if self.support_index(path).is_ok() {
            Some("signature support")
        } else if self.item_index(path).is_ok() {
            Some("item")
        } else {
            None
        };
        if let Some(existing) = existing {
            return Err(host::Error::DuplicateFacadeEntry {
                path: path.to_string().into(),
                existing,
                incoming: "signature support",
            });
        }
        if let Ok(index) = self.type_index(path) {
            let ordinary = &self.types[index];
            let exact = ordinary_identity.is_some_and(|identity| {
                ordinary.visibility() == host::type_def::Visibility::Public
                    && ordinary
                        .identity()
                        .imported()
                        .is_some_and(|imported| imported.as_bytes() == identity)
            });
            if !exact {
                return Err(host::Error::ConflictingHostIdentity {
                    existing: ordinary.path().into(),
                    incoming: path.to_string().into(),
                });
            }
        }
        let representation =
            definition
                .native_identity()
                .ok_or_else(|| host::Error::ConflictingHostIdentity {
                    existing: path.to_string().into(),
                    incoming: path.to_string().into(),
                })?;
        let contract = host::support::SignatureContract::new(
            path.clone(),
            role,
            site,
            profile,
            linked,
            representation,
        )?;
        let index = self
            .supports
            .binary_search_by(|candidate| candidate.path_ref().cmp(path))
            .expect_err("support vacancy is checked before insertion");
        self.supports
            .try_reserve_exact(1)
            .map_err(|_| host::Error::Allocation)?;
        self.supports.insert(index, contract.clone());
        Ok(host::support::Local::new(contract))
    }

    /// Seal one compiler/profile-authenticated support-only operation endpoint.
    #[cfg(feature = "rust-import")]
    #[doc(hidden)]
    #[allow(
        clippy::too_many_arguments,
        reason = "the operation-endpoint proof bundle is intentionally indivisible"
    )]
    pub fn profile_endpoint<T: 'static>(
        &mut self,
        site: &rawr_import::surface::facade::OperationSite,
        target: &str,
        profile: [u8; 32],
        source: [u8; 32],
        projection: [u8; 32],
        linked: [u8; 32],
        invoke: host::endpoint::Invoke,
    ) -> Result<(), host::Error> {
        if site.root() != self.root.segments()[0].as_str()
            || site.namespace() != rawr_import::surface::facade::Namespace::Value
            || site.path().is_empty()
        {
            return Err(host::Error::InvalidFacadeRoot(site.path().into()));
        }
        let relative = host::path::Path::parse(site.path())?;
        let path = host::path::Path::qualify(&self.root, &relative);
        let target = host::path::Path::qualify(&self.root, &host::path::Path::parse(target)?);
        if self
            .types
            .binary_search_by(|definition| definition.path_ref().cmp(&target))
            .is_err()
        {
            return Err(host::Error::UnknownFacadeType {
                item: path.to_string().into(),
                ty: target.to_string().into(),
            });
        }
        let index = self
            .endpoints
            .binary_search_by(|endpoint| endpoint.path_ref().cmp(&path));
        if index.is_ok() {
            return Err(host::Error::DuplicateFacadeEntry {
                path: path.to_string().into(),
                existing: "operation endpoint",
                incoming: "operation endpoint",
            });
        }
        let contract = host::endpoint::Contract::new(
            path,
            target,
            *site.selector(),
            profile,
            source,
            projection,
            linked,
            std::any::TypeId::of::<T>(),
            invoke,
        )?;
        self.endpoints
            .try_reserve_exact(1)
            .map_err(|_| host::Error::Allocation)?;
        self.endpoints.insert(
            index.expect_err("duplicate endpoint paths are rejected above"),
            contract,
        );
        Ok(())
    }

    /// Declare a host value type whose `==` is `T`'s own `PartialEq`;
    /// `capabilities` should name `PartialEq`.
    pub fn value_type<T: PartialEq + Clone + 'static>(
        &mut self,
        path: impl Into<ImmutableString>,
        parameters: &[&str],
        capabilities: &[capability::Kind],
    ) -> Result<&mut Self, host::Error> {
        host::value::register_equality::<T>();
        self.type_::<T>(path, parameters, capabilities)
    }

    /// [`value_type`](Self::value_type) whose `Default` builds the field of
    /// a derived `Default`; `capabilities` should include
    /// [`capability::Kind::Default`].
    pub fn default_value_type<T: PartialEq + Clone + Default + crate::func::SendSync + 'static>(
        &mut self,
        path: impl Into<ImmutableString>,
        parameters: &[&str],
        capabilities: &[capability::Kind],
    ) -> Result<&mut Self, host::Error> {
        host::value::register_default::<T>();
        self.value_type::<T>(path, parameters, capabilities)
    }

    /// Declare an opaque host type with a `Default`, which a derived
    /// `Default` builds, whose values have no `PartialEq`.
    pub fn default_type<T: Clone + Default + crate::func::SendSync + 'static>(
        &mut self,
        path: impl Into<ImmutableString>,
        parameters: &[&str],
        capabilities: &[capability::Kind],
    ) -> Result<&mut Self, host::Error> {
        host::value::register_default::<T>();
        self.type_::<T>(path, parameters, capabilities)
    }

    /// Let a State holding `T` values carry them into the next runner
    /// process after a `.rs` edit, as the bytes `T` writes.
    pub fn carried<T: host::value::Carry>(&mut self) -> &mut Self {
        host::value::register_carry::<T>();
        self
    }

    /// Declare a host type whose values must not be silently discarded.
    pub fn required_type<T: 'static>(
        &mut self,
        path: impl Into<ImmutableString>,
        parameters: &[&str],
        capabilities: &[capability::Kind],
    ) -> Result<&mut Self, host::Error> {
        let relative = host::path::Path::parse(path)?;
        let path = host::path::Path::qualify(&self.root, &relative);
        self.ensure_vacant(&path, "type")?;
        let definition = host::type_def::Definition::declared::<T>(
            path,
            parameters,
            capabilities,
            host::type_def::Storage::Owned,
            host::type_def::Role::Ordinary,
            host::usage::Policy::Required,
            host::type_def::Visibility::Public,
        )?;
        self.insert_type(definition)?;
        Ok(self)
    }

    /// Declare a host type restricted to one checked invocation scope.
    pub fn invocation_type<T: 'static>(
        &mut self,
        path: impl Into<ImmutableString>,
        parameters: &[&str],
    ) -> Result<&mut Self, host::Error> {
        let relative = host::path::Path::parse(path)?;
        let path = host::path::Path::qualify(&self.root, &relative);
        if !parameters.is_empty() {
            return Err(host::Error::InvocationTypeParameters {
                ty: path.to_string().into(),
            });
        }
        self.ensure_vacant(&path, "type")?;
        let definition = host::type_def::Definition::declared::<T>(
            path,
            parameters,
            &[],
            host::type_def::Storage::Invocation,
            host::type_def::Role::Ordinary,
            host::usage::Policy::Ordinary,
            host::type_def::Visibility::Public,
        )?;
        self.insert_type(definition)?;
        Ok(self)
    }

    /// Declare the facade's one-parameter durable stream subscription plan type.
    ///
    /// This role is compiler authority for lowering a direct
    /// [`StreamOperation`](host::item::Kind::StreamOperation) call. The Rust
    /// representation is Rawr's opaque [`host::stream::Plan`]; native stream
    /// factories never enter the value.
    pub fn stream_plan_type(
        &mut self,
        path: impl Into<ImmutableString>,
        parameter: &str,
    ) -> Result<&mut Self, host::Error> {
        let relative = host::path::Path::parse(path)?;
        let path = host::path::Path::qualify(&self.root, &relative);
        self.ensure_vacant(&path, "type")?;
        let definition = host::type_def::Definition::declared::<host::stream::Plan>(
            path,
            &[parameter],
            &[],
            host::type_def::Storage::Owned,
            host::type_def::Role::StreamPlan,
            host::usage::Policy::Required,
            host::type_def::Visibility::Public,
        )?;
        self.insert_type(definition)?;
        Ok(self)
    }

    /// Declare a trait that source names as `impl Trait<..>`: the type it
    /// [projects](Self::project) to, which every implementor converts into.
    pub fn trait_<T: 'static>(
        &mut self,
        path: impl Into<ImmutableString>,
        parameters: &[&str],
    ) -> Result<&mut Self, host::Error> {
        let relative = host::path::Path::parse(path)?;
        let path = host::path::Path::qualify(&self.root, &relative);
        self.ensure_vacant(&path, "type")?;
        let definition = host::type_def::Definition::declared::<T>(
            path,
            parameters,
            &[],
            host::type_def::Storage::Owned,
            host::type_def::Role::Trait,
            host::usage::Policy::Ordinary,
            host::type_def::Visibility::Public,
        )?;
        self.insert_type(definition)?;
        Ok(self)
    }

    /// Declare a trait, as [`trait_`](Self::trait_), that script types may
    /// implement: `declaration` is its Rust `trait` item, whose paths are
    /// absolute. A value of an implementing type converts into the trait's
    /// projection by that type's `new`, given the value, the impl's methods
    /// in declaration order and the default of each `Default` associated
    /// type.
    pub fn implementable_trait<T: 'static>(
        &mut self,
        path: impl Into<ImmutableString>,
        parameters: &[&str],
        declaration: &str,
    ) -> Result<&mut Self, host::Error> {
        let path = path.into();
        let relative = host::path::Path::parse(path.clone())?;
        if crate::parser::parse_trait(declaration).is_none_or(|declared| {
            relative.segments().last().map(|name| name.as_str())
                != Some(declared.name.text.as_str())
        }) {
            return Err(host::Error::InvalidTypePath(path));
        }
        self.trait_::<T>(path, parameters)?;
        let qualified = host::path::Path::qualify(&self.root, &relative);
        let index = self
            .type_index(&qualified)
            .expect("the trait was just declared");
        let definition = self.types.remove(index);
        self.types.insert(
            index,
            definition.with_shape(host::type_def::Shape::Trait(declaration.into())),
        );
        Ok(self)
    }

    /// Declare a compiler-inferred host type that checked source cannot name.
    pub fn private_type<T: 'static>(
        &mut self,
        path: impl Into<ImmutableString>,
        parameters: &[&str],
        capabilities: &[capability::Kind],
    ) -> Result<&mut Self, host::Error> {
        let relative = host::path::Path::parse(path)?;
        let path = host::path::Path::qualify(&self.root, &relative);
        self.ensure_vacant(&path, "type")?;
        let definition = host::type_def::Definition::declared::<T>(
            path,
            parameters,
            capabilities,
            host::type_def::Storage::Owned,
            host::type_def::Role::Ordinary,
            host::usage::Policy::Ordinary,
            host::type_def::Visibility::Private,
        )?;
        self.insert_type(definition)?;
        Ok(self)
    }

    /// Add a locally validated item declaration.
    pub fn item(&mut self, item: host::item::Definition) -> Result<&mut Self, host::Error> {
        let path = host::path::Path::qualify(&self.root, item.path_ref());
        if self.type_index(&path).is_ok() {
            return Err(host::Error::DuplicateFacadeEntry {
                path: path.to_string().into(),
                existing: "type",
                incoming: "item",
            });
        }
        if let Ok(index) = self.item_index(&path)
            && let Some(existing) = self.items[index]
                .1
                .iter()
                .find(|existing| same_namespace(existing, &item))
            && !compatible_overload(existing, &item)
        {
            return Err(host::Error::IncompatibleFacadeOverload {
                item: path.to_string().into(),
                existing_kind: existing.kind(),
                existing_receiver: existing.receiver(),
                incoming_kind: item.kind(),
                incoming_receiver: item.receiver(),
            });
        }
        match self.item_index(&path) {
            Ok(index) => {
                self.items[index]
                    .1
                    .try_reserve(1)
                    .map_err(|_| host::Error::Allocation)?;
                self.items[index].1.push(item);
            }
            Err(index) => {
                self.items
                    .try_reserve(1)
                    .map_err(|_| host::Error::Allocation)?;
                let mut declarations = Vec::new();
                declarations
                    .try_reserve_exact(1)
                    .map_err(|_| host::Error::Allocation)?;
                declarations.push(item);
                self.items.insert(index, (path, declarations));
            }
        }
        Ok(self)
    }

    /// Retain one already-resolved linked profile item in an imported facade.
    #[cfg(feature = "rust-import")]
    #[doc(hidden)]
    pub fn profile_item(
        &mut self,
        item: &host::item::Definition,
    ) -> Result<&mut Self, host::Error> {
        if item.path_ref().segments().first() != self.root.segments().first() {
            return Err(host::Error::InvalidFacadeRoot(item.path().into()));
        }
        self.profile_items
            .try_reserve_exact(1)
            .map_err(|_| host::Error::Allocation)?;
        self.profile_items.push(item.clone());
        Ok(self)
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn compatibility_item<A: host::ImportAllocation + ?Sized>(
        &mut self,
        item: &host::item::Definition,
        allocation: &mut A,
    ) -> Result<&mut Self, host::Error> {
        if item.path_ref().segments().first() != self.root.segments().first() {
            return Err(host::Error::InvalidFacadeRoot(item.path().into()));
        }
        allocation
            .retain_checked(Some(1), item.retained_bytes())
            .map_err(|_| host::Error::Allocation)?;
        self.profile_items
            .try_reserve_exact(1)
            .map_err(|_| host::Error::Allocation)?;
        self.profile_items
            .push(item.try_clone().map_err(|_| host::Error::Allocation)?);
        Ok(self)
    }

    /// Declare one direct projection between types in this facade.
    pub fn project(
        &mut self,
        source: impl Into<ImmutableString>,
        target: impl Into<ImmutableString>,
        arguments: &[&str],
    ) -> Result<&mut Self, host::Error> {
        let source = host::path::Path::qualify(&self.root, &host::path::Path::parse(source)?);
        let target = host::path::Path::qualify(&self.root, &host::path::Path::parse(target)?);
        self.projections
            .try_reserve(1)
            .map_err(|_| host::Error::Allocation)?;
        self.projections.push(ProjectionDeclaration {
            source,
            target: ProjectionTarget::Local(target),
            arguments: projection_arguments(arguments)?,
        });
        Ok(self)
    }

    /// Declare one direct projection to an exported type from another facade.
    pub fn project_to(
        &mut self,
        source: impl Into<ImmutableString>,
        target: host::type_def::Export,
        arguments: &[&str],
    ) -> Result<&mut Self, host::Error> {
        let source = host::path::Path::qualify(&self.root, &host::path::Path::parse(source)?);
        self.projections
            .try_reserve(1)
            .map_err(|_| host::Error::Allocation)?;
        self.projections.push(ProjectionDeclaration {
            source,
            target: ProjectionTarget::External(target),
            arguments: projection_arguments(arguments)?,
        });
        Ok(self)
    }

    /// Declare a direct projection with explicit source or fresh arguments.
    pub fn project_with(
        &mut self,
        source: impl Into<ImmutableString>,
        target: impl Into<ImmutableString>,
        arguments: &[host::type_def::Argument],
    ) -> Result<&mut Self, host::Error> {
        let source = host::path::Path::qualify(&self.root, &host::path::Path::parse(source)?);
        let target = host::path::Path::qualify(&self.root, &host::path::Path::parse(target)?);
        self.projections
            .try_reserve(1)
            .map_err(|_| host::Error::Allocation)?;
        self.projections.push(ProjectionDeclaration {
            source,
            target: ProjectionTarget::Local(target),
            arguments: clone_arguments(arguments)?,
        });
        Ok(self)
    }

    /// Declare a direct external projection with explicit source or fresh arguments.
    pub fn project_to_with(
        &mut self,
        source: impl Into<ImmutableString>,
        target: host::type_def::Export,
        arguments: &[host::type_def::Argument],
    ) -> Result<&mut Self, host::Error> {
        let source = host::path::Path::qualify(&self.root, &host::path::Path::parse(source)?);
        self.projections
            .try_reserve(1)
            .map_err(|_| host::Error::Allocation)?;
        self.projections.push(ProjectionDeclaration {
            source,
            target: ProjectionTarget::External(target),
            arguments: clone_arguments(arguments)?,
        });
        Ok(self)
    }

    /// Validate all cross-references and produce one atomic installable proof.
    pub fn finish(mut self) -> Result<Definition, host::Error> {
        validate_module_parents(&self.root, &self.modules)?;
        validate_type_parents(&self.root, &self.modules, &self.types)?;
        validate_type_identities(&self.types)?;
        resolve_projections(&mut self.types, self.projections)?;
        let imported = host::type_ref::ImportedTypes::from_facade(&self.types)?;

        let item_count = self
            .items
            .iter()
            .try_fold(0_usize, |count, (_, declarations)| {
                count.checked_add(declarations.len())
            })
            .ok_or(host::Error::Allocation)?;
        let mut items = Vec::new();
        items
            .try_reserve_exact(item_count)
            .map_err(|_| host::Error::Allocation)?;
        for (_, declarations) in &self.items {
            let mut resolved = Vec::new();
            resolved
                .try_reserve_exact(declarations.len())
                .map_err(|_| host::Error::Allocation)?;
            for item in declarations {
                resolved.push(item.resolve(
                    &self.root,
                    &self.modules,
                    &self.types,
                    &self.supports,
                    &imported,
                )?);
            }
            resolved.sort_by_key(host::item::Definition::fingerprint);
            for (index, item) in resolved.iter().enumerate() {
                for existing in &resolved[..index] {
                    if same_namespace(existing, item)
                        && existing.signature().same_inputs(item.signature())?
                    {
                        return Err(host::Error::DuplicateFacadeOverloadInputs {
                            item: item.path().into(),
                        });
                    }
                }
            }
            items.extend(resolved);
        }

        #[cfg(feature = "rust-import")]
        append_profile_items(&mut items, self.profile_items)?;
        let items = items.into_boxed_slice();
        // A re-export names one of this facade's types at another path, or a
        // builtin (`builtin_alias`).
        for (alias, target) in &self.aliases {
            let declared = |path: &host::path::Path| {
                self.types
                    .binary_search_by(|definition| definition.path_ref().cmp(path))
                    .is_ok()
            };
            // A re-exported variant names a variant of one of its types.
            if !host::is_builtin_alias_target(target)
                && !declared(target)
                && !target.parent().is_some_and(|owner| declared(&owner))
            {
                return Err(host::Error::DuplicateFacadeEntry {
                    path: alias.to_string().into(),
                    existing: "re-export of an unknown type",
                    incoming: "re-export",
                });
            }
        }
        self.aliases.sort();
        let mut modules = self.modules.clone();
        for (alias, target) in &self.aliases {
            // Re-exports join the proof under a spelling no module has.
            modules.push(host::path::Path::parse(format!("{alias}::{target}"))?);
        }
        let fingerprint = fingerprint(
            &self.root,
            self.abi,
            self.compatibility,
            &modules,
            &self.types,
            &self.supports,
            &self.endpoints,
            &items,
        )?;
        Ok(Definition {
            root: self.root,
            abi: self.abi,
            modules: self.modules.into_boxed_slice(),
            types: self.types.into_boxed_slice(),
            supports: self.supports.into_boxed_slice(),
            endpoints: self.endpoints.into_boxed_slice(),
            items,
            aliases: self.aliases.into_boxed_slice(),
            fingerprint,
        })
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn finish_imported<A: host::ImportAllocation + ?Sized>(
        mut self,
        allocation: &mut A,
    ) -> Result<Definition, host::Error> {
        validate_module_parents(&self.root, &self.modules)?;
        validate_type_parents(&self.root, &self.modules, &self.types)?;
        validate_type_identities(&self.types)?;
        resolve_projections(&mut self.types, self.projections)?;
        let imported =
            host::type_ref::ImportedTypes::from_imported_facade(&self.types, allocation)?;

        let item_count = self
            .items
            .iter()
            .try_fold(0_usize, |count, (_, declarations)| {
                count.checked_add(declarations.len())
            })
            .ok_or(host::Error::Allocation)?;
        allocation
            .retain_checked(
                Some(item_count),
                item_count.checked_mul(std::mem::size_of::<host::item::Definition>()),
            )
            .map_err(|_| host::Error::Allocation)?;
        let mut items = Vec::new();
        items
            .try_reserve_exact(item_count)
            .map_err(|_| host::Error::Allocation)?;
        for (_, declarations) in self.items {
            let start = items.len();
            for item in declarations {
                items.push(item.resolve_imported(
                    &self.root,
                    &self.modules,
                    &self.types,
                    &self.supports,
                    &imported,
                    allocation,
                )?);
            }
            items[start..].sort_by_key(host::item::Definition::fingerprint);
            for (offset, item) in items[start..].iter().enumerate() {
                for existing in &items[start..start + offset] {
                    if same_namespace(existing, item)
                        && existing.signature().same_inputs(item.signature())?
                    {
                        return Err(host::Error::DuplicateFacadeOverloadInputs {
                            item: item.path().into(),
                        });
                    }
                }
            }
        }

        append_profile_items(&mut items, self.profile_items)?;

        let (fingerprint_items, fingerprint_bytes) = fingerprint_usage(
            &self.root,
            &self.modules,
            &self.types,
            &self.supports,
            &self.endpoints,
            &items,
        )?;
        allocation
            .retain_checked(Some(fingerprint_items), Some(fingerprint_bytes))
            .map_err(|_| host::Error::Allocation)?;
        let items = items.into_boxed_slice();
        // A carried re-export names one of this facade's types at another
        // path, or a builtin (`builtin_alias`).
        for (alias, target) in &self.aliases {
            let declared = |path: &host::path::Path| {
                self.types
                    .binary_search_by(|definition| definition.path_ref().cmp(path))
                    .is_ok()
            };
            // A re-exported variant names a variant of one of its types.
            if !host::is_builtin_alias_target(target)
                && !declared(target)
                && !target.parent().is_some_and(|owner| declared(&owner))
            {
                return Err(host::Error::DuplicateFacadeEntry {
                    path: alias.to_string().into(),
                    existing: "re-export of an unknown type",
                    incoming: "re-export",
                });
            }
        }
        self.aliases.sort();
        let mut modules = Vec::new();
        modules
            .try_reserve_exact(self.modules.len() + self.aliases.len())
            .map_err(|_| host::Error::Allocation)?;
        modules.extend(self.modules.iter().cloned());
        for (alias, target) in &self.aliases {
            // Re-exports join the proof under a spelling no module has.
            modules.push(host::path::Path::parse(format!("{alias}::{target}"))?);
        }
        let fingerprint = fingerprint(
            &self.root,
            self.abi,
            self.compatibility,
            &modules,
            &self.types,
            &self.supports,
            &self.endpoints,
            &items,
        )?;
        Ok(Definition {
            root: self.root,
            abi: self.abi,
            modules: self.modules.into_boxed_slice(),
            types: self.types.into_boxed_slice(),
            supports: self.supports.into_boxed_slice(),
            endpoints: self.endpoints.into_boxed_slice(),
            items,
            aliases: self.aliases.into_boxed_slice(),
            fingerprint,
        })
    }

    fn ensure_vacant(
        &self,
        path: &host::path::Path,
        incoming: &'static str,
    ) -> Result<(), host::Error> {
        let existing = if self.modules.binary_search(path).is_ok() {
            Some("module")
        } else if self.type_index(path).is_ok() {
            Some("type")
        } else if self.support_index(path).is_ok() {
            Some("signature support")
        } else if self.item_index(path).is_ok() {
            Some("item")
        } else {
            None
        };
        if let Some(existing) = existing {
            return Err(host::Error::DuplicateFacadeEntry {
                path: path.to_string().into(),
                existing,
                incoming,
            });
        }
        Ok(())
    }

    fn type_index(&self, path: &host::path::Path) -> Result<usize, usize> {
        self.types
            .binary_search_by(|definition| definition.path_ref().cmp(path))
    }

    fn support_index(&self, path: &host::path::Path) -> Result<usize, usize> {
        self.supports
            .binary_search_by(|contract| contract.path_ref().cmp(path))
    }

    fn item_index(&self, path: &host::path::Path) -> Result<usize, usize> {
        self.items
            .binary_search_by(|(candidate, _)| candidate.cmp(path))
    }

    fn insert_type(&mut self, definition: host::type_def::Definition) -> Result<(), host::Error> {
        let path = definition.path_ref();
        let index = self
            .type_index(path)
            .expect_err("vacancy is checked before type insertion");
        self.types
            .try_reserve(1)
            .map_err(|_| host::Error::Allocation)?;
        self.types.insert(index, definition);
        Ok(())
    }
}

#[cfg(feature = "rust-import")]
fn append_profile_items(
    items: &mut Vec<host::item::Definition>,
    profile: Vec<host::item::Definition>,
) -> Result<(), host::Error> {
    items
        .try_reserve_exact(profile.len())
        .map_err(|_| host::Error::Allocation)?;
    for candidate in profile {
        let mut replaced = false;
        for existing in items.iter().filter(|existing| {
            existing.path_ref() == candidate.path_ref() && same_namespace(existing, &candidate)
        }) {
            if !compatible_overload(existing, &candidate) {
                return Err(host::Error::IncompatibleFacadeOverload {
                    item: candidate.path().into(),
                    existing_kind: existing.kind(),
                    existing_receiver: existing.receiver(),
                    incoming_kind: candidate.kind(),
                    incoming_receiver: candidate.receiver(),
                });
            }
            if existing.signature().same_inputs(candidate.signature())? {
                if existing.same_contract_except_imported(&candidate) {
                    replaced = true;
                    break;
                }
                return Err(host::Error::DuplicateFacadeOverloadInputs {
                    item: candidate.path().into(),
                });
            }
        }
        if !replaced {
            items.push(candidate);
        }
    }
    items.sort_by(|left, right| {
        left.path_ref()
            .cmp(right.path_ref())
            .then_with(|| left.fingerprint().cmp(&right.fingerprint()))
    });
    for (index, item) in items.iter().enumerate() {
        for existing in items[..index]
            .iter()
            .rev()
            .take_while(|existing| existing.path_ref() == item.path_ref())
            .filter(|existing| same_namespace(existing, item))
        {
            if !compatible_overload(existing, item) {
                return Err(host::Error::IncompatibleFacadeOverload {
                    item: item.path().into(),
                    existing_kind: existing.kind(),
                    existing_receiver: existing.receiver(),
                    incoming_kind: item.kind(),
                    incoming_receiver: item.receiver(),
                });
            }
            if existing.signature().same_inputs(item.signature())? {
                return Err(host::Error::DuplicateFacadeOverloadInputs {
                    item: item.path().into(),
                });
            }
        }
    }
    Ok(())
}

/// Rust keeps macros apart from values, so a macro and a function may share a
/// path without being overloads of each other: iced's `column![..]` and
/// `column(..)`. Overload rules apply within one namespace.
fn same_namespace(left: &host::item::Definition, right: &host::item::Definition) -> bool {
    left.kind().is_macro() == right.kind().is_macro()
}

/// Overloads share a kind and receiver, with two Rust-shaped exceptions. A
/// free value may share its path with free functions, as a Rust `fn` item is
/// both a value and callable: `button::primary` names the value,
/// `button::primary(theme, status)` calls. A field reader (a shared method
/// without parameters) may share its name with owned builder methods, as
/// iced's `style.border` and `style.border(..)`. A tuple variant's
/// constructor may share its name with the shared reader of its field.
fn compatible_overload(existing: &host::item::Definition, item: &host::item::Definition) -> bool {
    debug_assert!(same_namespace(existing, item));
    use host::item::Kind;
    let value = |item: &host::item::Definition| item.kind() == Kind::Value;
    let function = |item: &host::item::Definition| item.kind() == Kind::Function;
    let reader = |item: &host::item::Definition| {
        item.kind() == Kind::Method
            && item.receiver() == Some(crate::receiver::Mode::Shared)
            && item.signature().parameters().is_empty()
    };
    let builder = |item: &host::item::Definition| {
        item.kind() == Kind::Method && item.receiver() == Some(crate::receiver::Mode::Owned)
    };
    // A tuple variant's constructor and the reader of its field in a
    // pattern, as iced's `Key::Named`.
    let constructor = |item: &host::item::Definition| item.kind() == Kind::AssociatedFunction;
    existing.kind() == item.kind() && existing.receiver() == item.receiver()
        || value(existing) && function(item)
        || function(existing) && value(item)
        || reader(existing) && builder(item)
        || builder(existing) && reader(item)
        || constructor(existing) && reader(item)
        || reader(existing) && constructor(item)
}

impl Definition {
    /// Return the script-visible root namespace.
    #[must_use]
    pub fn root(&self) -> &str {
        self.root.name()
    }

    /// Return this facade's declared ABI version.
    #[must_use]
    pub const fn abi(&self) -> Abi {
        self.abi
    }

    /// Return the deterministic exact facade proof.
    #[must_use]
    pub const fn fingerprint(&self) -> &Fingerprint {
        &self.fingerprint
    }

    /// Iterate over the facade's registered types in canonical path order.
    pub fn types(&self) -> impl ExactSizeIterator<Item = &host::type_def::Definition> {
        self.types.iter()
    }

    /// The facade's re-exports, each a qualified path and the type it names.
    #[cfg(feature = "rust-import")]
    pub(crate) fn aliases(&self) -> &[(host::path::Path, host::path::Path)] {
        &self.aliases
    }

    /// Iterate over the facade's registered items in canonical path order.
    pub fn items(&self) -> impl ExactSizeIterator<Item = &host::item::Definition> {
        self.items.iter()
    }

    pub(crate) fn overloads(&self, path: &str) -> Option<&[host::item::Definition]> {
        let path = host::path::Path::parse(path).ok()?;
        item_group(&self.items, &path)
    }

    /// Export an opaque proof for a type declared by this finished facade.
    pub fn export_type(
        &self,
        path: impl Into<ImmutableString>,
    ) -> Result<host::type_def::Export, host::Error> {
        let path = host::path::Path::qualify(&self.root, &host::path::Path::parse(path)?);
        let definition = self
            .types
            .binary_search_by(|definition| definition.path_ref().cmp(&path))
            .ok()
            .map(|index| &self.types[index])
            .ok_or_else(|| host::Error::UnknownFacadeType {
                item: "<facade export>".into(),
                ty: path.to_string().into(),
            })?;
        Ok(host::type_def::Export::new(
            self.root.clone(),
            self.fingerprint.clone(),
            definition.contract(),
        ))
    }

    /// Export one signature-only type already sealed by this finished facade.
    #[cfg(feature = "rust-import")]
    #[doc(hidden)]
    pub fn support_type(
        &self,
        path: impl Into<ImmutableString>,
    ) -> Result<host::support::SupportExport, host::Error> {
        let path = host::path::Path::qualify(&self.root, &host::path::Path::parse(path)?);
        let contract = self
            .supports
            .binary_search_by(|contract| contract.path_ref().cmp(&path))
            .ok()
            .map(|index| self.supports[index].clone())
            .ok_or_else(|| host::Error::UnknownFacadeType {
                item: "<facade support export>".into(),
                ty: path.to_string().into(),
            })?;
        Ok(host::support::SupportExport::new(
            self.root.clone(),
            self.fingerprint.clone(),
            contract,
        ))
    }

    pub(crate) const fn root_path(&self) -> &host::path::Path {
        &self.root
    }

    pub(crate) fn into_registry_parts(self) -> RegistryParts {
        RegistryParts {
            root: self.root,
            modules: self.modules,
            types: self.types,
            supports: self.supports,
            endpoints: self.endpoints,
            items: self.items,
            aliases: self.aliases,
            fingerprint: self.fingerprint,
        }
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn registry_usage(&self) -> (Option<usize>, Option<usize>) {
        let item_count = self.items.len();
        let imported_aliases = self
            .types
            .iter()
            .filter(|definition| definition.identity().imported().is_some())
            .count();
        let imported_groups = self
            .types
            .iter()
            .enumerate()
            .filter(|(index, definition)| {
                let Some(identity) = definition.identity().imported() else {
                    return false;
                };
                !self.types[..*index]
                    .iter()
                    .any(|prior| prior.identity().imported() == Some(identity))
            })
            .count();
        let items = Some(2_usize)
            .and_then(|items| items.checked_add(self.modules.len()))
            .and_then(|items| items.checked_add(self.types.len()))
            .and_then(|items| items.checked_add(self.supports.len()))
            .and_then(|items| items.checked_add(self.endpoints.len()))
            .and_then(|items| items.checked_add(item_count));
        let items = items
            .and_then(|items| items.checked_add(imported_aliases))
            .and_then(|items| items.checked_add(imported_groups));
        let module_count = self.modules.len().checked_add(1);
        let bytes = module_count
            .and_then(|modules| modules.checked_mul(std::mem::size_of::<host::path::Path>()))
            .and_then(|bytes| {
                bytes.checked_add(
                    self.types
                        .len()
                        .checked_mul(std::mem::size_of::<host::type_def::Definition>())?,
                )
            })
            .and_then(|bytes| {
                self.supports.iter().try_fold(bytes, |bytes, support| {
                    bytes.checked_add(support.retained_bytes()?)
                })
            })
            .and_then(|bytes| {
                self.endpoints.iter().try_fold(bytes, |bytes, endpoint| {
                    bytes.checked_add(endpoint.retained_bytes()?)
                })
            })
            .and_then(|bytes| {
                bytes.checked_add(
                    self.items
                        .len()
                        .checked_mul(std::mem::size_of::<host::item::Definition>())?,
                )
            })
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<host::facade::Dependency>()))
            .and_then(|bytes| bytes.checked_add(self.root.display_len()?))
            .and_then(|bytes| {
                bytes.checked_add(imported_aliases.checked_mul(std::mem::size_of::<usize>())?)
            })
            .and_then(|bytes| {
                bytes.checked_add(imported_groups.checked_mul(host::imported_index_bytes())?)
            });
        (items, bytes)
    }

    pub(crate) const fn modules(&self) -> &[host::path::Path] {
        &self.modules
    }

    pub(crate) const fn type_map(&self) -> &[host::type_def::Definition] {
        &self.types
    }

    pub(crate) const fn support_map(&self) -> &[host::support::SignatureContract] {
        &self.supports
    }

    pub(crate) const fn endpoint_map(&self) -> &[host::endpoint::Contract] {
        &self.endpoints
    }

    pub(crate) fn item_sets(
        &self,
    ) -> impl Iterator<Item = (&host::path::Path, &[host::item::Definition])> {
        let mut offset = 0;
        std::iter::from_fn(move || {
            let first = self.items.get(offset)?;
            let path = first.path_ref();
            let start = offset;
            offset += 1;
            while self
                .items
                .get(offset)
                .is_some_and(|item| item.path_ref() == path)
            {
                offset += 1;
            }
            Some((path, &self.items[start..offset]))
        })
    }

    pub(crate) fn dependencies(&self) -> BTreeSet<Dependency> {
        let mut dependencies = BTreeSet::new();
        for definition in &self.types {
            dependencies.extend(
                definition
                    .projections()
                    .iter()
                    .filter_map(host::type_def::Projection::target_dependency),
            );
        }
        for item in &self.items {
            dependencies.extend(
                item.signature()
                    .external_types()
                    .into_iter()
                    .map(host::type_def::Export::dependency),
            );
            dependencies.extend(
                item.signature()
                    .support_exports()
                    .into_iter()
                    .map(host::support::SupportExport::dependency),
            );
        }
        dependencies.remove(&Dependency::new(&self.root, self.fingerprint.clone()));
        dependencies
    }
}

impl Dependency {
    pub(crate) fn new(root: &host::path::Path, fingerprint: Fingerprint) -> Self {
        Self {
            root: root.to_string().into(),
            fingerprint,
        }
    }

    pub(crate) fn try_new(
        root: &host::path::Path,
        fingerprint: Fingerprint,
    ) -> Result<Self, host::Error> {
        let bytes = root.display_len().ok_or(host::Error::Allocation)?;
        let mut display = String::new();
        display
            .try_reserve_exact(bytes)
            .map_err(|_| host::Error::Allocation)?;
        use std::fmt::Write as _;
        write!(&mut display, "{root}").map_err(|_| host::Error::Allocation)?;
        Ok(Self {
            root: display.into(),
            fingerprint,
        })
    }

    /// Return the canonical facade root.
    #[must_use]
    pub fn root(&self) -> &str {
        &self.root
    }

    /// Return the exact finished facade fingerprint.
    #[must_use]
    pub const fn fingerprint(&self) -> &Fingerprint {
        &self.fingerprint
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn retained_bytes(&self) -> Option<usize> {
        let fingerprint = self
            .fingerprint
            .retained_bytes()?
            .checked_sub(std::mem::size_of::<Fingerprint>())?;
        std::mem::size_of::<Self>()
            .checked_add(self.root.len())?
            .checked_add(fingerprint)
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn try_clone(&self) -> Result<Self, rawr_import::surface::Error> {
        Ok(Self {
            root: self.root.clone(),
            fingerprint: self.fingerprint.try_clone()?,
        })
    }
}

impl Fingerprint {
    #[cfg(feature = "rust-import")]
    pub(super) const COLLECTION_OVERHEAD: usize = shared_slice_overhead::<ImmutableString>()
        + shared_slice_overhead::<TypeProof>()
        + shared_slice_overhead::<host::support::SignatureContract>()
        + shared_slice_overhead::<host::endpoint::Contract>()
        + shared_slice_overhead::<host::item::Fingerprint>();

    /// Return bounded canonical digest evidence for compatibility composition.
    #[cfg(feature = "rust-import")]
    #[doc(hidden)]
    #[must_use]
    pub fn compatibility_digest(&self) -> [u8; 16] {
        use crate::preview::digest::Write as _;

        let mut writer =
            crate::preview::digest::Writer::with_version(b"rawr-facade-compatibility-seed-v1");
        self.write(&mut writer)
            .expect("validated facade fingerprints have canonical digest evidence");
        writer.finish().bytes()
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn retained_bytes(&self) -> Option<usize> {
        let modules = self.modules.iter().try_fold(
            self.modules
                .len()
                .checked_mul(std::mem::size_of::<ImmutableString>())?,
            |bytes, module| bytes.checked_add(module.len()),
        )?;
        let types = self
            .types
            .iter()
            .try_fold(0_usize, |bytes, ty| bytes.checked_add(ty.retained_bytes()?))?;
        let supports = self.supports.iter().try_fold(0_usize, |bytes, support| {
            bytes.checked_add(support.retained_bytes()?)
        })?;
        let endpoints = self.endpoints.iter().try_fold(0_usize, |bytes, endpoint| {
            bytes.checked_add(endpoint.retained_bytes()?)
        })?;
        let items = self.items.iter().try_fold(0_usize, |bytes, item| {
            bytes.checked_add(item.retained_bytes()?)
        })?;
        std::mem::size_of::<Self>()
            .checked_add(Self::COLLECTION_OVERHEAD)?
            .checked_add(self.root.len())?
            .checked_add(modules)?
            .checked_add(types)?
            .checked_add(supports)?
            .checked_add(endpoints)?
            .checked_add(items)
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn try_clone(&self) -> Result<Self, rawr_import::surface::Error> {
        // All collections belong to the finished immutable facade. Cloning
        // retains that authority; semantic normalization detaches before edits.
        Ok(self.clone())
    }
}

impl ProfileCycle {
    const SCHEMA: u32 = 3;

    /// Transform one finished exact facade plus its structured compatibility
    /// preimage outputs into a non-authoritative self-cycle proof.
    #[doc(hidden)]
    pub fn transform(
        definition: &Definition,
        exact: [u8; 32],
        semantic: [u8; 32],
    ) -> Result<Self, host::Error> {
        if definition.fingerprint.compatibility != Some(exact) || exact == semantic {
            return Err(host::Error::InvalidFacadeProfileCycle {
                root: definition.root().into(),
            });
        }
        Ok(Self {
            root: definition.root().into(),
            abi: definition.abi,
            exact,
            semantic,
            schema: Self::SCHEMA,
        })
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn accepts(&self, fingerprint: &Fingerprint) -> bool {
        self.schema == Self::SCHEMA
            && fingerprint.root == self.root
            && fingerprint.abi == self.abi
            && fingerprint.compatibility == Some(self.exact)
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn finalization<'a>(
        &'a self,
        fingerprint: &'a Fingerprint,
    ) -> Option<ProfileFinalization<'a>> {
        self.accepts(fingerprint).then_some(ProfileFinalization {
            cycle: self,
            exact: fingerprint,
            runtime_representations_are_valid: core::cell::Cell::new(true),
            normalized: core::cell::RefCell::default(),
            companions: core::cell::RefCell::default(),
        })
    }
}

#[cfg(feature = "rust-import")]
impl ProfileFinalization<'_> {
    pub(crate) fn reject_runtime_representation(&self) {
        self.runtime_representations_are_valid.set(false);
    }

    pub(crate) fn runtime_representations_are_valid(&self) -> bool {
        self.runtime_representations_are_valid.get()
    }

    fn token(&self, path: &host::path::Path) -> host::type_def::import::Identity {
        let mut digest = Sha256::new();
        digest.update(b"rawr.application.profile-type-finalization/v1");
        digest.update(self.cycle.semantic);
        digest.update(
            u64::try_from(path.segments().len())
                .unwrap_or(u64::MAX)
                .to_le_bytes(),
        );
        for segment in path.segments() {
            digest.update(
                u64::try_from(segment.len())
                    .unwrap_or(u64::MAX)
                    .to_le_bytes(),
            );
            digest.update(segment.as_bytes());
        }
        host::type_def::import::Identity::new(digest.finalize().into())
    }

    /// The path of the one type proof, in the exact facade or a companion,
    /// whose contract `matches`.
    fn proven(
        &self,
        matches: impl Fn(&host::type_def::Contract) -> bool,
    ) -> Option<host::path::Path> {
        let companions = self.companions.borrow();
        let mut proofs = self
            .exact
            .types
            .iter()
            .chain(
                companions
                    .iter()
                    .flat_map(|companion| companion.types.iter()),
            )
            .filter(|proof| matches(&proof.contract));
        let proof = proofs.next()?;
        proofs
            .next()
            .is_none()
            .then(|| proof.contract.path_ref().clone())
    }

    pub(crate) fn normalize_identity(
        &self,
        path: &host::path::Path,
        identity: &mut host::type_def::Identity,
    ) -> bool {
        let host::type_def::Identity::Linked(linked) = identity else {
            return false;
        };
        let linked = host::type_def::Identity::Linked(*linked);
        if self
            .proven(|contract| contract.path_ref() == path && contract.identity_ref() == &linked)
            .is_none()
        {
            return false;
        }
        *identity = host::type_def::Identity::Imported(self.token(path));
        true
    }

    pub(crate) fn normalize_runtime_identity(
        &self,
        path: &str,
        identity: std::any::TypeId,
    ) -> Option<host::type_def::import::Identity> {
        self.proven(|contract| {
            contract.path() == path && contract.runtime_identity() == Some(identity)
        })
        .map(|path| self.token(&path))
    }

    pub(crate) fn normalize_identity_text(
        &self,
        path: &str,
        identity: &mut host::type_def::Identity,
    ) -> bool {
        let host::type_def::Identity::Linked(linked) = identity else {
            return false;
        };
        let linked = host::type_def::Identity::Linked(*linked);
        let Some(path) =
            self.proven(|contract| contract.path() == path && contract.identity_ref() == &linked)
        else {
            return false;
        };
        *identity = host::type_def::Identity::Imported(self.token(&path));
        true
    }

    pub(crate) fn normalize_fingerprint(&self, fingerprint: &mut Fingerprint) -> usize {
        let reused = self
            .normalized
            .borrow()
            .iter()
            .find(|(input, ..)| input == fingerprint)
            .map(|(_, output, completed)| (output.clone(), *completed));
        if let Some((output, completed)) = reused {
            *fingerprint = output;
            return completed;
        }
        let input = fingerprint.clone();
        let mut completed = 0_usize;
        if fingerprint == self.exact {
            fingerprint.compatibility = Some(self.cycle.semantic);
            completed = completed.saturating_add(1);
        } else if fingerprint.root != self.cycle.root
            && fingerprint.compatibility == Some(self.cycle.exact)
        {
            self.companions.borrow_mut().push(input.clone());
            fingerprint.compatibility = Some(self.cycle.semantic);
            completed = completed.saturating_add(1);
        }
        for proof in Shared::make_mut(&mut fingerprint.types).iter_mut() {
            completed = completed.saturating_add(proof.contract.normalize_profile(self));
            for projection in &mut proof.projections {
                completed = completed.saturating_add(projection.normalize_profile(self));
            }
        }
        for support in Shared::make_mut(&mut fingerprint.supports).iter_mut() {
            completed = completed.saturating_add(support.normalize_profile(self));
        }
        for endpoint in Shared::make_mut(&mut fingerprint.endpoints).iter_mut() {
            completed = completed.saturating_add(endpoint.normalize_profile(self));
        }
        for item in Shared::make_mut(&mut fingerprint.items).iter_mut() {
            completed = completed.saturating_add(item.normalize_profile(self));
        }
        fingerprint.seal();
        self.normalized
            .borrow_mut()
            .push((input, fingerprint.clone(), completed));
        completed
    }

    pub(crate) fn normalize_dependency(&self, dependency: &mut Dependency) -> (bool, usize) {
        let direct = dependency.root == self.cycle.root && &dependency.fingerprint == self.exact;
        let completed = self.normalize_fingerprint(&mut dependency.fingerprint);
        (direct, completed)
    }
}

impl Fingerprint {
    fn hash_with<H: Hasher>(&self, compatibility: Option<[u8; 32]>, state: &mut H) {
        self.root.hash(state);
        self.abi.hash(state);
        compatibility.hash(state);
        self.content.hash(state);
    }

    /// Recompute the content digest after the slices are set.
    fn seal(&mut self) {
        let mut content = ContentHasher(Sha256::new());
        self.modules.hash(&mut content);
        self.types.hash(&mut content);
        self.supports.hash(&mut content);
        self.endpoints.hash(&mut content);
        self.items.hash(&mut content);
        self.content = content.0.finalize().into();
    }
}

impl PartialEq for Fingerprint {
    fn eq(&self, other: &Self) -> bool {
        // The content digest seals the contract slices, as `Hash` reads it,
        // so equal digests are equal contracts. A program's proof and the
        // runtime registry's are separate copies, so walking the slices
        // cost every attested call a walk of the whole facade.
        self.content == other.content
            && self.root == other.root
            && self.abi == other.abi
            && self.compatibility == other.compatibility
    }
}

impl Ord for Fingerprint {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Equality recognizes shared nested type proofs; ordering otherwise
        // walks their complete shapes even when the facade is unchanged.
        if self == other {
            return std::cmp::Ordering::Equal;
        }
        (
            &self.root,
            &self.abi,
            &self.compatibility,
            &self.modules,
            &self.types,
            &self.supports,
            &self.endpoints,
            &self.items,
        )
            .cmp(&(
                &other.root,
                &other.abi,
                &other.compatibility,
                &other.modules,
                &other.types,
                &other.supports,
                &other.endpoints,
                &other.items,
            ))
    }
}

impl PartialOrd for Fingerprint {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Hash for Fingerprint {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.hash_with(self.compatibility, state);
    }
}

impl TypeProof {
    #[cfg(feature = "rust-import")]
    fn retained_bytes(&self) -> Option<usize> {
        let contract = self
            .contract
            .retained_bytes()?
            .checked_sub(std::mem::size_of::<host::type_def::Contract>())?;
        let projections = self
            .projections
            .iter()
            .try_fold(0_usize, |bytes, projection| {
                bytes.checked_add(projection.retained_bytes()?)
            })?;
        std::mem::size_of::<Self>()
            .checked_add(contract)?
            .checked_add(projections)
    }
}

impl crate::preview::digest::Write for Fingerprint {
    fn write(
        &self,
        writer: &mut crate::preview::digest::Writer,
    ) -> Result<(), crate::preview::digest::Error> {
        use crate::preview::digest::write_slice;

        writer.tag(0x2900);
        writer.tag(0x2901);
        writer.text(&self.root);
        writer.tag(0x2902);
        writer.u32(self.abi.get());
        if let Some(compatibility) = self.compatibility {
            writer.tag(0x2908);
            writer.bytes(&compatibility);
        }
        writer.tag(0x2903);
        writer.count(self.modules.len());
        for module in self.modules.iter() {
            writer.text(module);
        }
        writer.tag(0x2904);
        write_slice(writer, &self.types)?;
        writer.tag(0x2905);
        write_slice(writer, &self.supports)?;
        writer.tag(0x2906);
        write_slice(writer, &self.endpoints)?;
        writer.tag(0x2907);
        write_slice(writer, &self.items)?;
        Ok(())
    }
}

impl crate::preview::digest::Write for Dependency {
    fn write(
        &self,
        writer: &mut crate::preview::digest::Writer,
    ) -> Result<(), crate::preview::digest::Error> {
        writer.tag(0x2a00);
        writer.tag(0x2a01);
        writer.text(&self.root);
        writer.tag(0x2a02);
        self.fingerprint.write(writer)?;
        Ok(())
    }
}

impl crate::preview::digest::Write for TypeProof {
    fn write(
        &self,
        writer: &mut crate::preview::digest::Writer,
    ) -> Result<(), crate::preview::digest::Error> {
        use crate::preview::digest::write_slice;

        writer.tag(0x2b00);
        writer.tag(0x2b01);
        self.contract.write(writer)?;
        writer.tag(0x2b02);
        write_slice(writer, &self.projections)?;
        Ok(())
    }
}

fn validate_module_parents(
    root: &host::path::Path,
    modules: &[host::path::Path],
) -> Result<(), host::Error> {
    for module in modules {
        let parent = module
            .parent()
            .expect("qualified facade modules retain their root");
        if &parent != root && modules.binary_search(&parent).is_err() {
            return Err(host::Error::MissingFacadeModule {
                path: module.to_string().into(),
                module: parent.to_string().into(),
            });
        }
    }
    Ok(())
}

fn validate_type_parents(
    root: &host::path::Path,
    modules: &[host::path::Path],
    types: &[host::type_def::Definition],
) -> Result<(), host::Error> {
    for definition in types {
        let path = definition.path_ref();
        let parent = path
            .parent()
            .expect("qualified facade types retain their root");
        // A type under a type is an enum's struct variant, as
        // `HandleShape::Rectangle { .. }`: a builder of its own.
        if &parent != root
            && modules.binary_search(&parent).is_err()
            && !types.iter().any(|owner| owner.path_ref() == &parent)
        {
            return Err(host::Error::MissingFacadeModule {
                path: path.to_string().into(),
                module: parent.to_string().into(),
            });
        }
    }
    Ok(())
}

fn validate_type_identities(types: &[host::type_def::Definition]) -> Result<(), host::Error> {
    for (index, definition) in types.iter().enumerate() {
        for existing in &types[..index] {
            if existing.identity() == definition.identity()
                && !existing.same_identity_contract(definition)
            {
                return Err(host::Error::ConflictingHostIdentity {
                    existing: existing.path().into(),
                    incoming: definition.path().into(),
                });
            }
        }
    }
    Ok(())
}

fn projection_arguments(
    arguments: &[&str],
) -> Result<Box<[host::type_def::Argument]>, host::Error> {
    let mut resolved = Vec::new();
    resolved
        .try_reserve_exact(arguments.len())
        .map_err(|_| host::Error::Allocation)?;
    for argument in arguments {
        resolved.push(host::type_def::Argument::source(*argument)?);
    }
    Ok(resolved.into_boxed_slice())
}

fn clone_arguments(
    arguments: &[host::type_def::Argument],
) -> Result<Box<[host::type_def::Argument]>, host::Error> {
    let mut cloned = Vec::new();
    cloned
        .try_reserve_exact(arguments.len())
        .map_err(|_| host::Error::Allocation)?;
    cloned.extend(arguments.iter().cloned());
    Ok(cloned.into_boxed_slice())
}

fn resolve_projections(
    types: &mut [host::type_def::Definition],
    declarations: Vec<ProjectionDeclaration>,
) -> Result<(), host::Error> {
    if declarations.is_empty() {
        return Ok(());
    }
    let mut contracts = Vec::new();
    contracts
        .try_reserve_exact(types.len())
        .map_err(|_| host::Error::Allocation)?;
    for definition in types.iter() {
        contracts.push((definition.path_ref().clone(), definition.contract()));
    }
    let mut resolved = Vec::<(host::path::Path, Vec<host::type_def::Projection>)>::new();
    let mut edges = Vec::<(host::path::Path, Vec<host::path::Path>)>::new();
    let mut duplicates = Vec::<(host::path::Path, String)>::new();

    for declaration in declarations {
        let Some(source) = sorted_value(&contracts, &declaration.source) else {
            return Err(host::Error::UnknownProjectionSource {
                source: declaration.source.to_string().into(),
            });
        };
        let (target_path, target_parameters, projection) = match declaration.target {
            ProjectionTarget::Local(target) => {
                let Some(contract) = sorted_value(&contracts, &target) else {
                    return Err(host::Error::UnknownFacadeType {
                        item: declaration.source.to_string().into(),
                        ty: target.to_string().into(),
                    });
                };
                push_grouped_path(&mut edges, declaration.source.clone(), target.clone())?;
                (
                    target.to_string(),
                    contract.parameters().len(),
                    host::type_def::Projection::local(
                        contract.clone(),
                        declaration.arguments.clone(),
                    ),
                )
            }
            ProjectionTarget::External(target) => (
                target.path(),
                target.parameters().len(),
                host::type_def::Projection::external(target, declaration.arguments.clone()),
            ),
        };

        if target_parameters != declaration.arguments.len() {
            return Err(host::Error::InvalidFacadeTypeArguments {
                item: declaration.source.to_string().into(),
                ty: target_path.into(),
                expected: target_parameters,
                actual: declaration.arguments.len(),
            });
        }
        for argument in &declaration.arguments {
            let host::type_def::Argument::Source(parameter) = argument else {
                continue;
            };
            if !source.parameters().contains(parameter) {
                return Err(host::Error::InvalidProjectionParameter {
                    source: declaration.source.to_string().into(),
                    target: projection.target().into(),
                    parameter: parameter.clone(),
                });
            }
        }
        let duplicate = (declaration.source.clone(), projection.target());
        match duplicates.binary_search(&duplicate) {
            Ok(_) => {
                return Err(host::Error::DuplicateProjection {
                    source: duplicate.0.to_string().into(),
                    target: duplicate.1.into(),
                });
            }
            Err(index) => {
                duplicates
                    .try_reserve(1)
                    .map_err(|_| host::Error::Allocation)?;
                duplicates.insert(index, duplicate);
            }
        }
        push_grouped_projection(&mut resolved, declaration.source, projection)?;
    }

    reject_projection_cycles(&edges)?;
    for (source, mut projections) in resolved {
        projections.sort();
        let index = types
            .binary_search_by(|definition| definition.path_ref().cmp(&source))
            .expect("projection sources were validated against this type map");
        types[index].set_projections(projections.into_boxed_slice());
    }
    Ok(())
}

#[cfg(all(test, feature = "rust-import"))]
mod imported_allocation_tests {
    use super::{Abi, Builder};
    use crate::{host, Shared};
    use std::any::TypeId;

    #[test]
    fn fingerprint_order_matches_lexicographic_fields_with_shared_proofs() {
        fn original(left: &super::Fingerprint, right: &super::Fingerprint) -> std::cmp::Ordering {
            left.root
                .cmp(&right.root)
                .then_with(|| left.abi.cmp(&right.abi))
                .then_with(|| left.compatibility.cmp(&right.compatibility))
                .then_with(|| left.modules.cmp(&right.modules))
                .then_with(|| left.types.cmp(&right.types))
                .then_with(|| left.supports.cmp(&right.supports))
                .then_with(|| left.endpoints.cmp(&right.endpoints))
                .then_with(|| left.items.cmp(&right.items))
        }

        fn fixture(
            form: host::type_def::Form,
            leaf: host::type_ref::Builtin,
        ) -> super::Fingerprint {
            let mut builder = Builder::new("ordered", Abi::new(1).unwrap()).unwrap();
            builder.compatibility_seal([1; 32]).unwrap();
            builder.module("nested").unwrap();
            builder
                .imported_type(
                    "nested::Marker",
                    host::type_def::import::Identity::new([2; 32]),
                    &[],
                    Box::default(),
                    Box::default(),
                    host::type_def::Shape::Struct {
                        form,
                        constructor: None,
                        fields: Box::default(),
                        constructible: false,
                    }
                    .into(),
                    &mut host::TestImportAllocation::default(),
                )
                .unwrap();
            let nested = host::type_ref::Type::callable(
                host::type_ref::Type::tuple([
                    host::type_ref::Type::builtin(host::type_ref::Builtin::String),
                    host::type_ref::Type::builtin(leaf),
                ])
                .unwrap(),
                host::type_ref::Type::registered("nested::Marker").unwrap(),
            );
            builder
                .item(host::item::Definition::value("nested::value", nested).unwrap())
                .unwrap();
            let mut proof = builder.finish().unwrap().fingerprint().clone();
            proof.supports = vec![host::support::SignatureContract::new(
                host::path::Path::parse("ordered::Support").unwrap(),
                "support",
                [3; 32],
                [4; 32],
                [5; 32],
                TypeId::of::<Support>(),
            )
            .unwrap()]
            .into_boxed_slice()
            .into();
            proof.endpoints = vec![host::endpoint::Contract::new(
                host::path::Path::parse("ordered::endpoint").unwrap(),
                host::path::Path::parse("ordered::Support").unwrap(),
                [3; 32],
                [4; 32],
                [5; 32],
                [6; 32],
                [7; 32],
                TypeId::of::<Support>(),
                crate::Dynamic::from,
            )
            .unwrap()]
            .into_boxed_slice()
            .into();
            proof
        }

        let base = fixture(host::type_def::Form::Record, host::type_ref::Builtin::I64);
        let shared = base.clone();
        let independent = fixture(host::type_def::Form::Record, host::type_ref::Builtin::I64);
        let fallible = base.try_clone().unwrap();
        macro_rules! shared_collection {
            ($field:ident) => {
                assert!(Shared::ptr_eq(&base.$field, &shared.$field));
                assert!(Shared::ptr_eq(&base.$field, &fallible.$field));
                assert!(!Shared::ptr_eq(&base.$field, &independent.$field));
            };
        }
        shared_collection!(modules);
        shared_collection!(types);
        shared_collection!(supports);
        shared_collection!(endpoints);
        shared_collection!(items);
        assert!(std::ptr::eq(
            base.types[0].contract.shape(),
            shared.types[0].contract.shape(),
        ));
        assert!(!std::ptr::eq(
            base.types[0].contract.shape(),
            independent.types[0].contract.shape(),
        ));
        assert_eq!(base, shared);
        assert_eq!(base, independent);
        // Equality reads the content digest. Copies that share no slice are
        // equal, and one leaf deep in an item's signature, a tuple element
        // of a callable's parameter, sets two apart.
        let deep = fixture(host::type_def::Form::Record, host::type_ref::Builtin::Bool);
        assert_eq!(
            (&deep.root, deep.abi, deep.compatibility, &deep.modules),
            (&base.root, base.abi, base.compatibility, &base.modules)
        );
        assert_ne!(base, deep);
        assert_ne!(original(&base, &deep), std::cmp::Ordering::Equal);

        let mut variants = vec![base.clone(); 8];
        variants[0].root = "different".into();
        variants[1].abi = Abi::new(2).unwrap();
        variants[2].compatibility = None;
        variants[3].modules = Shared::default();
        variants[4].types =
            fixture(host::type_def::Form::Tuple, host::type_ref::Builtin::I64).types;
        variants[5].supports = Shared::default();
        variants[6].endpoints = Shared::default();
        variants[7].items =
            fixture(host::type_def::Form::Record, host::type_ref::Builtin::Bool).items;
        for variant in &variants {
            assert_ne!(original(&base, variant), std::cmp::Ordering::Equal);
        }
        // Fields are private; only normalization mutates them in production,
        // and it reseals the content digest the same way.
        for variant in &mut variants {
            variant.seal();
        }
        variants.extend([base, shared, independent]);
        let hash = |proof: &super::Fingerprint| {
            use std::hash::{Hash, Hasher};
            let mut state = std::collections::hash_map::DefaultHasher::new();
            proof.hash(&mut state);
            state.finish()
        };
        for left in &variants {
            for right in &variants {
                assert_eq!(left == right, hash(left) == hash(right));
            }
        }
        for (left_index, left) in variants.iter().enumerate() {
            for (right_index, right) in variants.iter().enumerate() {
                let expected = original(left, right);
                assert_eq!(left.cmp(right), expected, "{left_index} vs {right_index}");
                assert_eq!(
                    left.partial_cmp(right),
                    Some(expected),
                    "{left_index} vs {right_index}"
                );
                assert_eq!(left == right, expected.is_eq());
            }
        }
        let mut expected = variants.clone();
        expected.sort_by(original);
        variants.reverse();
        variants.sort();
        assert_eq!(variants, expected);
        variants.dedup();
        expected.dedup_by(|left, right| original(left, right).is_eq());
        assert_eq!(variants, expected);
        assert_eq!(variants.len(), 9);
    }

    #[test]
    fn facade_registry_usage_is_checked_and_deterministic() {
        let mut builder = Builder::new("fixture", Abi::new(1).expect("nonzero ABI")).unwrap();
        builder.module("nested").unwrap();
        builder.type_::<Marker>("nested::Marker", &[], &[]).unwrap();
        let definition = builder.finish().unwrap();
        let first = definition.registry_usage();
        let second = definition.registry_usage();

        assert_eq!(first, second);
        assert!(matches!(first, (Some(items), Some(bytes)) if items > 0 && bytes > 0));
    }

    #[test]
    fn facade_storage_is_sorted_without_duplicate_lookup_paths() {
        fn build(reverse: bool) -> super::Definition {
            let mut builder = Builder::new("ordered", Abi::new(1).expect("nonzero ABI")).unwrap();
            let entries = if reverse { ["z", "a"] } else { ["a", "z"] };
            for entry in entries {
                builder.module(entry).unwrap();
                let path = format!("{entry}::Marker");
                builder.type_::<Marker>(path, &[], &[]).unwrap();
                builder
                    .item(
                        host::item::Definition::value(
                            format!("{entry}::value"),
                            host::type_ref::Type::builtin(host::type_ref::Builtin::I64),
                        )
                        .unwrap(),
                    )
                    .unwrap();
            }
            builder.finish().unwrap()
        }

        let forward = build(false);
        let reverse = build(true);
        assert_eq!(forward.fingerprint(), reverse.fingerprint());
        assert_eq!(
            forward
                .types()
                .map(host::type_def::Definition::path)
                .collect::<Vec<_>>(),
            ["ordered::a::Marker", "ordered::z::Marker"]
        );
        assert_eq!(
            forward
                .items()
                .map(host::item::Definition::path)
                .collect::<Vec<_>>(),
            ["ordered::a::value", "ordered::z::value"]
        );
        assert_eq!(
            std::mem::size_of_val(&forward.types),
            std::mem::size_of::<Box<[host::type_def::Definition]>>()
        );
    }

    #[test]
    fn signature_support_coexists_only_with_the_exact_imported_identity() {
        fn linked() -> super::Definition {
            let mut builder = Builder::new("proof", Abi::new(1).expect("nonzero ABI")).unwrap();
            builder.type_::<Support>("Opaque", &[], &[]).unwrap();
            builder.finish().unwrap()
        }

        fn imported(identity: [u8; 32]) -> Builder {
            let mut builder = Builder::new("proof", Abi::new(2).expect("nonzero ABI")).unwrap();
            let mut allocation = host::TestImportAllocation::default();
            let shape: Shared<host::type_def::Shape> = host::type_def::Shape::Opaque.into();
            builder
                .imported_type(
                    "Opaque",
                    host::type_def::import::Identity::new(identity),
                    &[],
                    Box::default(),
                    Box::default(),
                    shape,
                    &mut allocation,
                )
                .unwrap();
            builder
        }

        let linked = linked();
        let definition = linked
            .types()
            .find(|definition| definition.path() == "proof::Opaque")
            .expect("the linked representation exists");
        let mut semantic = Builder::new("proof", Abi::new(2).expect("nonzero ABI")).unwrap();
        semantic
            .fixture_profile_semantic_support_type(definition, "opaque", [1; 32], [2; 32], [3; 32])
            .expect("one exact semantic/support pair installs atomically");
        assert!(matches!(
            semantic.fixture_profile_semantic_support_type(
                definition, "opaque", [1; 32], [2; 32], [3; 32],
            ),
            Err(host::Error::DuplicateFacadeEntry {
                existing: "type",
                ..
            })
        ));
        let semantic = semantic.finish().unwrap();
        let ordinary = semantic
            .types()
            .find(|candidate| candidate.path() == "proof::Opaque")
            .expect("the semantic ordinary definition exists");
        assert_eq!(ordinary, definition);
        assert!(semantic.export_type("Opaque").is_ok());
        assert!(semantic.support_type("Opaque").is_ok());

        let identity = [0x51; 32];
        let mut exact = imported(identity);
        exact
            .insert_profile_support_type(
                definition,
                "opaque",
                [1; 32],
                [2; 32],
                [3; 32],
                Some(&identity),
            )
            .expect("the exact imported identity admits separate support");
        let exact = exact.finish().unwrap();
        assert_eq!(exact.types().len(), 1);
        assert!(exact.export_type("Opaque").is_ok());
        assert!(exact.support_type("Opaque").is_ok());

        let mut wrong = imported([0x52; 32]);
        assert!(matches!(
            wrong.insert_profile_support_type(
                definition,
                "opaque",
                [1; 32],
                [2; 32],
                [3; 32],
                Some(&identity),
            ),
            Err(host::Error::ConflictingHostIdentity { .. })
        ));

        let mut linked_ordinary = Builder::new("proof", Abi::new(2).unwrap()).unwrap();
        linked_ordinary.type_::<Other>("Opaque", &[], &[]).unwrap();
        assert!(matches!(
            linked_ordinary.insert_profile_support_type(
                definition,
                "opaque",
                [1; 32],
                [2; 32],
                [3; 32],
                Some(&identity),
            ),
            Err(host::Error::ConflictingHostIdentity { .. })
        ));

        let mut duplicate = imported(identity);
        duplicate
            .insert_profile_support_type(
                definition,
                "opaque",
                [1; 32],
                [2; 32],
                [3; 32],
                Some(&identity),
            )
            .unwrap();
        assert!(matches!(
            duplicate.insert_profile_support_type(
                definition,
                "opaque",
                [1; 32],
                [2; 32],
                [3; 32],
                Some(&identity),
            ),
            Err(host::Error::DuplicateFacadeEntry {
                existing: "signature support",
                ..
            })
        ));
    }

    #[test]
    fn profile_finalization_reconciles_only_authenticated_stage_local_identities() {
        const EXACT: [u8; 32] = [0x41; 32];
        const SEMANTIC: [u8; 32] = [0x52; 32];

        fn profile<T: 'static>(extra_module: bool) -> super::Definition {
            let mut builder = Builder::new("proof", Abi::new(1).unwrap()).unwrap();
            builder.compatibility_seal(EXACT).unwrap();
            if extra_module {
                builder.module("extra").unwrap();
            }
            builder.type_::<T>("Opaque", &[], &[]).unwrap();
            builder.finish().unwrap()
        }

        let provisional = profile::<Support>(false);
        let admitted = profile::<Other>(false);
        assert_ne!(provisional.fingerprint(), admitted.fingerprint());

        let provisional_cycle =
            super::ProfileCycle::transform(&provisional, EXACT, SEMANTIC).unwrap();
        let admitted_cycle = super::ProfileCycle::transform(&admitted, EXACT, SEMANTIC).unwrap();
        let provisional_finalization = provisional_cycle
            .finalization(provisional.fingerprint())
            .unwrap();
        let admitted_finalization = admitted_cycle.finalization(admitted.fingerprint()).unwrap();
        let exact_digest = provisional.fingerprint().compatibility_digest();
        let mut provisional_fingerprint = provisional.fingerprint().clone();
        let mut admitted_fingerprint = admitted.fingerprint().clone();

        assert_eq!(
            provisional_finalization.normalize_fingerprint(&mut provisional_fingerprint),
            2
        );
        assert_eq!(
            admitted_finalization.normalize_fingerprint(&mut admitted_fingerprint),
            2
        );
        assert_eq!(provisional_fingerprint, admitted_fingerprint);
        assert_eq!(
            provisional.fingerprint().compatibility_digest(),
            exact_digest
        );
        assert_eq!(provisional.fingerprint().compatibility, Some(EXACT));
        assert!(!Shared::ptr_eq(
            &provisional.fingerprint().types,
            &provisional_fingerprint.types,
        ));
        assert!(Shared::ptr_eq(
            &provisional.fingerprint().modules,
            &provisional_fingerprint.modules,
        ));

        let path = provisional.types().next().unwrap().path_ref().clone();
        let mut foreign = host::type_def::Identity::linked(TypeId::of::<Other>());
        assert!(!provisional_finalization.normalize_identity(&path, &mut foreign));
        assert_eq!(foreign.native(), Some(TypeId::of::<Other>()));

        let mut exact = host::type_def::Identity::linked(TypeId::of::<Support>());
        let foreign_path = host::path::Path::parse("proof::Foreign").unwrap();
        assert!(!provisional_finalization.normalize_identity(&foreign_path, &mut exact));
        assert_eq!(exact.native(), Some(TypeId::of::<Support>()));
    }

    #[test]
    fn profile_finalization_reconciles_a_companion_sealed_with_the_exact_seal() {
        const EXACT: [u8; 32] = [0x85; 32];
        const SEMANTIC: [u8; 32] = [0x96; 32];

        fn companion<T: 'static>(seal: Option<[u8; 32]>) -> super::Fingerprint {
            let mut builder = Builder::new("companion", Abi::new(1).unwrap()).unwrap();
            if let Some(seal) = seal {
                builder.compatibility_seal(seal).unwrap();
            }
            builder.type_::<T>("Opaque", &[], &[]).unwrap();
            builder.finish().unwrap().fingerprint().clone()
        }

        let mut exact = Builder::new("proof", Abi::new(1).unwrap()).unwrap();
        exact.compatibility_seal(EXACT).unwrap();
        let exact = exact.finish().unwrap();
        let cycle = super::ProfileCycle::transform(&exact, EXACT, SEMANTIC).unwrap();
        let normalized = |mut fingerprint: super::Fingerprint| {
            let finalization = cycle.finalization(exact.fingerprint()).unwrap();
            finalization.normalize_fingerprint(&mut fingerprint);
            fingerprint
        };

        assert_eq!(
            normalized(companion::<Support>(Some(EXACT))),
            normalized(companion::<Other>(Some(EXACT)))
        );
        assert_ne!(
            normalized(companion::<Support>(None)),
            normalized(companion::<Other>(None))
        );
    }

    #[test]
    fn profile_finalization_preserves_ordinary_facade_structure() {
        const EXACT: [u8; 32] = [0x63; 32];
        const SEMANTIC: [u8; 32] = [0x74; 32];

        fn profile<T: 'static>(extra_module: bool) -> super::Definition {
            let mut builder = Builder::new("proof", Abi::new(1).unwrap()).unwrap();
            builder.compatibility_seal(EXACT).unwrap();
            if extra_module {
                builder.module("extra").unwrap();
            }
            builder.type_::<T>("Opaque", &[], &[]).unwrap();
            builder.finish().unwrap()
        }

        let provisional = profile::<Support>(false);
        let changed = profile::<Other>(true);
        let provisional_cycle =
            super::ProfileCycle::transform(&provisional, EXACT, SEMANTIC).unwrap();
        let changed_cycle = super::ProfileCycle::transform(&changed, EXACT, SEMANTIC).unwrap();
        let provisional_finalization = provisional_cycle
            .finalization(provisional.fingerprint())
            .unwrap();
        let changed_finalization = changed_cycle.finalization(changed.fingerprint()).unwrap();
        let mut provisional_fingerprint = provisional.fingerprint().clone();
        let mut changed_fingerprint = changed.fingerprint().clone();

        provisional_finalization.normalize_fingerprint(&mut provisional_fingerprint);
        changed_finalization.normalize_fingerprint(&mut changed_fingerprint);

        assert_ne!(provisional_fingerprint, changed_fingerprint);
        assert_eq!(changed_fingerprint.modules.as_ref(), ["proof::extra"]);
    }

    #[test]
    fn profile_finalization_normalizes_a_repeated_facade_once_with_the_same_result() {
        const EXACT: [u8; 32] = [0x17; 32];
        const SEMANTIC: [u8; 32] = [0x28; 32];

        let mut builder = Builder::new("proof", Abi::new(1).unwrap()).unwrap();
        builder.compatibility_seal(EXACT).unwrap();
        builder.type_::<Support>("Opaque", &[], &[]).unwrap();
        let facade = builder.finish().unwrap();
        let cycle = super::ProfileCycle::transform(&facade, EXACT, SEMANTIC).unwrap();

        // Full: every occurrence through its own fresh finalization.
        let mut full = facade.fingerprint().clone();
        let full_completed = cycle
            .finalization(facade.fingerprint())
            .unwrap()
            .normalize_fingerprint(&mut full);

        // Incremental: repeated occurrences through one finalization.
        let finalization = cycle.finalization(facade.fingerprint()).unwrap();
        let mut first = facade.fingerprint().clone();
        let mut second = facade.fingerprint().clone();
        assert_eq!(
            finalization.normalize_fingerprint(&mut first),
            full_completed
        );
        assert_eq!(
            finalization.normalize_fingerprint(&mut second),
            full_completed
        );
        assert_eq!(first, full);
        assert_eq!(second, full);
        assert!(Shared::ptr_eq(&first.types, &second.types));
        let digest = |fingerprint: &super::Fingerprint| {
            let mut hasher = super::ContentHasher(<super::Sha256 as sha2::Digest>::new());
            std::hash::Hash::hash(fingerprint, &mut hasher);
            <[u8; 32]>::from(sha2::Digest::finalize(hasher.0))
        };
        assert_eq!(digest(&second), digest(&full));
    }

    #[cfg(feature = "rust-import")]
    #[test]
    fn profile_finalization_rejects_bad_pairs_and_keeps_arbitrary_host_exact() {
        const EXACT: [u8; 32] = [0x85; 32];
        const SEMANTIC: [u8; 32] = [0x96; 32];

        let mut builder = Builder::new("proof", Abi::new(1).unwrap()).unwrap();
        builder.compatibility_seal(EXACT).unwrap();
        builder.type_::<Support>("Opaque", &[], &[]).unwrap();
        let facade = builder.finish().unwrap();
        let cycle = super::ProfileCycle::transform(&facade, EXACT, SEMANTIC).unwrap();

        let valid_finalization = cycle.finalization(facade.fingerprint()).unwrap();
        let mut valid = crate::host::runtime::type_ref::Type::builtin(
            "String",
            TypeId::of::<crate::ImmutableString>(),
        );
        let exact = valid.clone();
        assert_eq!(valid.normalize_profile(&valid_finalization), 1);
        assert!(valid_finalization.runtime_representations_are_valid());
        assert_eq!(exact.runtime(), TypeId::of::<crate::ImmutableString>());

        let invalid_finalization = cycle.finalization(facade.fingerprint()).unwrap();
        let mut invalid =
            crate::host::runtime::type_ref::Type::builtin("String", TypeId::of::<crate::INT>());
        assert_eq!(invalid.normalize_profile(&invalid_finalization), 0);
        assert!(!invalid_finalization.runtime_representations_are_valid());

        let host_finalization = cycle.finalization(facade.fingerprint()).unwrap();
        let mut arbitrary_host = crate::host::runtime::type_ref::Type::host(
            "foreign::Opaque".into(),
            TypeId::of::<Other>(),
            Box::default(),
            None,
        );
        assert_eq!(arbitrary_host.normalize_profile(&host_finalization), 0);
        assert!(host_finalization.runtime_representations_are_valid());
        assert_eq!(arbitrary_host.runtime(), TypeId::of::<Other>());
    }

    struct Marker;
    struct Other;
    struct Support;
}

fn sorted_value<'a, T>(
    entries: &'a [(host::path::Path, T)],
    path: &host::path::Path,
) -> Option<&'a T> {
    entries
        .binary_search_by(|(candidate, _)| candidate.cmp(path))
        .ok()
        .map(|index| &entries[index].1)
}

fn item_group<'a>(
    items: &'a [host::item::Definition],
    path: &host::path::Path,
) -> Option<&'a [host::item::Definition]> {
    let start = items.partition_point(|item| item.path_ref() < path);
    let end = items.partition_point(|item| item.path_ref() <= path);
    (start != end).then_some(&items[start..end])
}

fn push_grouped_path(
    groups: &mut Vec<(host::path::Path, Vec<host::path::Path>)>,
    source: host::path::Path,
    target: host::path::Path,
) -> Result<(), host::Error> {
    let index = match groups.binary_search_by(|(candidate, _)| candidate.cmp(&source)) {
        Ok(index) => index,
        Err(index) => {
            groups.try_reserve(1).map_err(|_| host::Error::Allocation)?;
            groups.insert(index, (source, Vec::new()));
            index
        }
    };
    let targets = &mut groups[index].1;
    if let Err(index) = targets.binary_search(&target) {
        targets
            .try_reserve(1)
            .map_err(|_| host::Error::Allocation)?;
        targets.insert(index, target);
    }
    Ok(())
}

fn push_grouped_projection(
    groups: &mut Vec<(host::path::Path, Vec<host::type_def::Projection>)>,
    source: host::path::Path,
    projection: host::type_def::Projection,
) -> Result<(), host::Error> {
    let index = match groups.binary_search_by(|(candidate, _)| candidate.cmp(&source)) {
        Ok(index) => index,
        Err(index) => {
            groups.try_reserve(1).map_err(|_| host::Error::Allocation)?;
            groups.insert(index, (source, Vec::new()));
            index
        }
    };
    groups[index]
        .1
        .try_reserve(1)
        .map_err(|_| host::Error::Allocation)?;
    groups[index].1.push(projection);
    Ok(())
}

fn reject_projection_cycles(
    edges: &[(host::path::Path, Vec<host::path::Path>)],
) -> Result<(), host::Error> {
    fn visit(
        node: &host::path::Path,
        edges: &[(host::path::Path, Vec<host::path::Path>)],
        active: &mut Vec<host::path::Path>,
        complete: &mut Vec<host::path::Path>,
    ) -> Result<(), host::Error> {
        if complete.binary_search(node).is_ok() {
            return Ok(());
        }
        let active_index = match active.binary_search(node) {
            Ok(_) => {
                return Err(host::Error::ProjectionCycle {
                    ty: node.to_string().into(),
                });
            }
            Err(index) => index,
        };
        active.try_reserve(1).map_err(|_| host::Error::Allocation)?;
        active.insert(active_index, node.clone());
        if let Some(targets) = sorted_value(edges, node) {
            for target in targets {
                visit(target, edges, active, complete)?;
            }
        }
        let active_index = active
            .binary_search(node)
            .expect("active projection node remains installed");
        active.remove(active_index);
        let complete_index = complete
            .binary_search(node)
            .expect_err("completed projection node is visited once");
        complete
            .try_reserve(1)
            .map_err(|_| host::Error::Allocation)?;
        complete.insert(complete_index, node.clone());
        Ok(())
    }

    let mut active = Vec::new();
    let mut complete = Vec::new();
    for (source, _) in edges {
        visit(source, edges, &mut active, &mut complete)?;
    }
    Ok(())
}

fn fingerprint(
    root: &host::path::Path,
    abi: Abi,
    compatibility: Option<[u8; 32]>,
    modules: &[host::path::Path],
    types: &[host::type_def::Definition],
    supports: &[host::support::SignatureContract],
    endpoints: &[host::endpoint::Contract],
    items: &[host::item::Definition],
) -> Result<Fingerprint, host::Error> {
    let mut module_proofs = Vec::new();
    module_proofs
        .try_reserve_exact(modules.len())
        .map_err(|_| host::Error::Allocation)?;
    for path in modules {
        module_proofs.push(path.to_string().into());
    }
    let mut type_proofs = Vec::new();
    type_proofs
        .try_reserve_exact(types.len())
        .map_err(|_| host::Error::Allocation)?;
    for definition in types {
        #[cfg(feature = "rust-import")]
        {
            let mut projections = Vec::new();
            projections
                .try_reserve_exact(definition.projections().len())
                .map_err(|_| host::Error::Allocation)?;
            for projection in definition.projections() {
                projections.push(
                    projection
                        .try_clone()
                        .map_err(|_| host::Error::Allocation)?,
                );
            }
            type_proofs.push(TypeProof {
                contract: definition
                    .try_contract()
                    .map_err(|_| host::Error::Allocation)?,
                projections: projections.into_boxed_slice(),
            });
        }
        #[cfg(not(feature = "rust-import"))]
        type_proofs.push(TypeProof {
            contract: definition.contract(),
            projections: definition.projections().into(),
        });
    }
    let mut item_proofs = Vec::new();
    item_proofs
        .try_reserve_exact(items.len())
        .map_err(|_| host::Error::Allocation)?;
    for item in items {
        #[cfg(feature = "rust-import")]
        item_proofs.push(
            item.try_fingerprint()
                .map_err(|_| host::Error::Allocation)?,
        );
        #[cfg(not(feature = "rust-import"))]
        item_proofs.push(item.fingerprint());
    }
    let mut fingerprint = Fingerprint {
        content: [0; 32],
        root: root.to_string().into(),
        abi,
        compatibility,
        modules: module_proofs.into_boxed_slice().into(),
        types: type_proofs.into_boxed_slice().into(),
        supports: supports.to_vec().into_boxed_slice().into(),
        endpoints: endpoints.to_vec().into_boxed_slice().into(),
        items: item_proofs.into_boxed_slice().into(),
    };
    fingerprint.seal();
    Ok(fingerprint)
}

// Logical Rc/Arc control storage, padded to the element alignment. Allocator
// rounding is excluded consistently with the other retained-byte estimates.
#[cfg(feature = "rust-import")]
const fn shared_slice_overhead<T>() -> usize {
    let header = 2 * std::mem::size_of::<usize>();
    let alignment = std::mem::align_of::<T>();
    header.div_ceil(alignment) * alignment
}

#[cfg(feature = "rust-import")]
fn fingerprint_usage(
    root: &host::path::Path,
    modules: &[host::path::Path],
    types: &[host::type_def::Definition],
    supports: &[host::support::SignatureContract],
    endpoints: &[host::endpoint::Contract],
    items: &[host::item::Definition],
) -> Result<(usize, usize), host::Error> {
    // Converting the prepared slices into shared storage briefly retains both
    // payload allocations. Admit that peak before construction starts.
    let conversion_payload = modules
        .len()
        .checked_mul(std::mem::size_of::<ImmutableString>())
        .and_then(|bytes| {
            bytes.checked_add(types.len().checked_mul(std::mem::size_of::<TypeProof>())?)
        })
        .and_then(|bytes| {
            bytes.checked_add(
                supports
                    .len()
                    .checked_mul(std::mem::size_of::<host::support::SignatureContract>())?,
            )
        })
        .and_then(|bytes| {
            bytes.checked_add(
                endpoints
                    .len()
                    .checked_mul(std::mem::size_of::<host::endpoint::Contract>())?,
            )
        })
        .and_then(|bytes| {
            bytes.checked_add(
                items
                    .len()
                    .checked_mul(std::mem::size_of::<host::item::Fingerprint>())?,
            )
        })
        .ok_or(host::Error::Allocation)?;
    let projection_count = types.iter().try_fold(0_usize, |count, definition| {
        count.checked_add(definition.projections().len())
    });
    let retained_items = Some(1_usize)
        .and_then(|count| count.checked_add(modules.len()))
        .and_then(|count| count.checked_add(types.len()))
        .and_then(|count| count.checked_add(supports.len()))
        .and_then(|count| count.checked_add(endpoints.len()))
        .and_then(|count| count.checked_add(projection_count?))
        .and_then(|count| count.checked_add(items.len()))
        .ok_or(host::Error::Allocation)?;
    let modules = modules.iter().try_fold(
        modules
            .len()
            .checked_mul(std::mem::size_of::<ImmutableString>())
            .ok_or(host::Error::Allocation)?,
        |bytes, module| bytes.checked_add(module.display_len()?),
    );
    let types = types.iter().try_fold(0_usize, |bytes, definition| {
        bytes.checked_add(definition.fingerprint_retained_bytes()?)
    });
    let supports = supports.iter().try_fold(0_usize, |bytes, support| {
        bytes.checked_add(support.retained_bytes()?)
    });
    let endpoints = endpoints.iter().try_fold(0_usize, |bytes, endpoint| {
        bytes.checked_add(endpoint.retained_bytes()?)
    });
    let items = items.iter().try_fold(0_usize, |bytes, item| {
        bytes.checked_add(item.fingerprint_retained_bytes()?)
    });
    let retained_bytes = std::mem::size_of::<Fingerprint>()
        .checked_add(Fingerprint::COLLECTION_OVERHEAD)
        .and_then(|bytes| bytes.checked_add(conversion_payload))
        .and_then(|bytes| bytes.checked_add(root.display_len()?))
        .and_then(|bytes| bytes.checked_add(modules?))
        .and_then(|bytes| bytes.checked_add(types?))
        .and_then(|bytes| bytes.checked_add(supports?))
        .and_then(|bytes| bytes.checked_add(endpoints?))
        .and_then(|bytes| bytes.checked_add(items?))
        .ok_or(host::Error::Allocation)?;
    Ok((retained_items, retained_bytes))
}
