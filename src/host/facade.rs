//! Atomic static host-facade definitions.

use crate::{capability, host, ImmutableString, Shared};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};

/// A non-zero facade ABI version.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
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

/// One opaque, fully validated static facade contract. A copy shares the
/// contract: the compiler and a runtime bound to it read one (#516).
#[derive(Clone, Debug)]
pub struct Definition(Shared<Body>);

impl std::ops::Deref for Definition {
    type Target = Body;

    fn deref(&self) -> &Body {
        &self.0
    }
}

/// A [`Definition`]'s contents.
#[derive(Clone, Debug)]
#[doc(hidden)]
pub struct Body {
    root: host::path::Path,
    abi: Abi,
    modules: Box<[host::path::Path]>,
    types: Box<[host::type_def::Definition]>,
    items: Box<[host::item::Definition]>,
    aliases: Box<[(host::path::Path, host::path::Path)]>,
    /// The modules reached at another path, sorted: each path and the
    /// module's path its entries are below (`Builder::imported_reexport`).
    reexports: Box<[(host::path::Path, host::path::Path)]>,
    type_aliases: Box<[TypeAlias]>,
    trait_headers: Box<[TraitHeader]>,
    impl_headers: Box<[ImplHeader]>,
    /// The crate's derive macros, as serde's `Serialize`, sorted.
    derives: Box<[host::path::Path]>,
    /// The imported methods that share a path and inputs, kept apart from
    /// the items until a use settles one (`host::family`). A generated
    /// runner binds each by its own selector.
    #[cfg(feature = "rust-import")]
    families: Box<[host::family::Pending]>,
    fingerprint: Fingerprint,
    /// The facade as its runner's image holds it, which builds each entry
    /// a lookup reaches (`host::image`): the slices above stay empty but
    /// for its re-exports.
    #[cfg(feature = "rust-import")]
    held: Option<host::image::Held>,
}

/// A crate's type alias, as `pub type Result = std::result::Result<(),
/// Error>;` or `pub type Element<'a, Message, Theme = Plain> =
/// element::Element<'a, Message, Theme>;`: its qualified path, its type
/// parameters with their defaults, and the type it names, in which each
/// parameter is a type parameter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TypeAlias {
    path: host::path::Path,
    parameters: Box<[ImmutableString]>,
    defaults: Box<[Option<host::type_ref::Type>]>,
    target: host::type_ref::Type,
}

impl TypeAlias {
    /// The crate's type alias at `path`, qualified, resolved against the
    /// declarations of its facade at `root`.
    #[cfg(feature = "rust-import")]
    pub(crate) fn imported(
        root: &host::path::Path,
        path: host::path::Path,
        parameters: Box<[ImmutableString]>,
        defaults: Box<[Option<host::type_ref::Type>]>,
        target: host::type_ref::Type,
        declared: &dyn host::declared::Declared,
    ) -> Result<Self, host::Error> {
        Self {
            path,
            parameters,
            defaults,
            target,
        }
        .resolve(root, declared)
    }

    pub(crate) const fn path(&self) -> &host::path::Path {
        &self.path
    }

    pub(crate) fn parameters(&self) -> &[ImmutableString] {
        &self.parameters
    }

    /// Each parameter's default, by position; empty when none has one.
    pub(crate) fn defaults(&self) -> &[Option<host::type_ref::Type>] {
        &self.defaults
    }

    pub(crate) const fn target(&self) -> &host::type_ref::Type {
        &self.target
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn try_clone(&self) -> Result<Self, rawr_import::surface::Error> {
        Ok(Self {
            path: self.path.try_clone()?,
            parameters: self.parameters.clone(),
            defaults: self
                .defaults
                .iter()
                .map(|default| {
                    default
                        .as_ref()
                        .map(host::type_ref::Type::try_clone)
                        .transpose()
                })
                .collect::<Result<_, _>>()?,
            target: self.target.try_clone()?,
        })
    }

    /// This alias with its target and defaults resolved against the
    /// facade's own types.
    fn resolve(
        &self,
        root: &host::path::Path,
        declared: &dyn host::declared::Declared,
    ) -> Result<Self, host::Error> {
        let mut parameters = self
            .parameters
            .iter()
            .map(|name| {
                Ok((
                    host::path::Path::parse(name.clone())?,
                    crate::capability::Set::from_explicit([]),
                ))
            })
            .collect::<Result<Vec<_>, host::Error>>()?;
        parameters.sort_by(|(left, _), (right, _)| left.cmp(right));
        let resolve =
            |ty: &host::type_ref::Type| ty.resolve(root, declared, &parameters, &self.path);
        Ok(Self {
            path: self.path.clone(),
            parameters: self.parameters.clone(),
            defaults: self
                .defaults
                .iter()
                .map(|default| default.as_ref().map(resolve).transpose())
                .collect::<Result<_, _>>()?,
            target: resolve(&self.target)?,
        })
    }

    /// The pseudo-module this alias joins a facade's proof as, under a
    /// spelling no module has: its path and a digest of what it names.
    fn proof_path(&self) -> Result<host::path::Path, host::Error> {
        use std::hash::{Hash as _, Hasher as _};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        format!(
            "{:?}|{:?}|{:?}",
            self.parameters, self.defaults, self.target
        )
        .hash(&mut hasher);
        host::path::Path::parse(format!("{}::alias_{:016x}", self.path, hasher.finish()))
    }
}

/// A crate trait Rawr code names (D604): its qualified path, identity,
/// type parameters other than `Self`, and each one's default.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TraitHeader {
    pub(crate) path: host::path::Path,
    pub(crate) identity: [u8; 32],
    pub(crate) parameters: Box<[ImmutableString]>,
    pub(crate) defaults: Box<[Option<host::type_ref::Type>]>,
}

/// One impl of a crate trait (D604): its own type parameters, the trait's
/// identity and type arguments other than `Self`, the self type, its
/// where-clause bounds as (type, trait identity, trait arguments), and
/// those that fix an associated type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ImplHeader {
    pub(crate) parameters: Box<[ImmutableString]>,
    pub(crate) trait_identity: [u8; 32],
    pub(crate) arguments: Box<[host::type_ref::Type]>,
    pub(crate) self_type: host::type_ref::Type,
    pub(crate) bounds: Box<[ImplBound]>,
    pub(crate) projections: Box<[ImplProjection]>,
}

/// A where-clause bound of an [`ImplHeader`] that fixes an associated type,
/// as `Renderer: image::Renderer<Handle = Handle>`: `<ty as Trait>::Name`
/// is `value`, and is each case's second type where `ty` is its first, as
/// the crate's impls give it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ImplProjection {
    pub(crate) ty: host::type_ref::Type,
    pub(crate) value: host::type_ref::Type,
    pub(crate) cases: Box<[(host::type_ref::Type, host::type_ref::Type)]>,
}

/// A where-clause bound of an [`ImplHeader`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ImplBound {
    pub(crate) ty: host::type_ref::Type,
    pub(crate) trait_identity: [u8; 32],
    pub(crate) arguments: Box<[host::type_ref::Type]>,
}

#[cfg(feature = "rust-import")]
fn clone_types(
    types: &[host::type_ref::Type],
) -> Result<Box<[host::type_ref::Type]>, rawr_import::surface::Error> {
    types.iter().map(host::type_ref::Type::try_clone).collect()
}

#[cfg(feature = "rust-import")]
impl TraitHeader {
    pub(crate) fn try_clone(&self) -> Result<Self, rawr_import::surface::Error> {
        Ok(Self {
            path: self.path.try_clone()?,
            identity: self.identity,
            parameters: self.parameters.clone(),
            defaults: self
                .defaults
                .iter()
                .map(|default| {
                    default
                        .as_ref()
                        .map(host::type_ref::Type::try_clone)
                        .transpose()
                })
                .collect::<Result<_, _>>()?,
        })
    }
}

#[cfg(feature = "rust-import")]
impl ImplHeader {
    pub(crate) fn try_clone(&self) -> Result<Self, rawr_import::surface::Error> {
        Ok(Self {
            parameters: self.parameters.clone(),
            trait_identity: self.trait_identity,
            arguments: clone_types(&self.arguments)?,
            self_type: self.self_type.try_clone()?,
            bounds: self
                .bounds
                .iter()
                .map(|bound| {
                    Ok(ImplBound {
                        ty: bound.ty.try_clone()?,
                        trait_identity: bound.trait_identity,
                        arguments: clone_types(&bound.arguments)?,
                    })
                })
                .collect::<Result<_, rawr_import::surface::Error>>()?,
            projections: self
                .projections
                .iter()
                .map(|projection| {
                    Ok(ImplProjection {
                        ty: projection.ty.try_clone()?,
                        value: projection.value.try_clone()?,
                        cases: projection
                            .cases
                            .iter()
                            .map(|(this, value)| Ok((this.try_clone()?, value.try_clone()?)))
                            .collect::<Result<_, rawr_import::surface::Error>>()?,
                    })
                })
                .collect::<Result<_, rawr_import::surface::Error>>()?,
        })
    }
}

#[cfg(feature = "rust-import")]
impl TraitHeader {
    /// This header with its defaults resolved against the declarations of
    /// the facade at `root`: `None` when one names a type it lacks.
    pub(crate) fn resolve(
        &self,
        root: &host::path::Path,
        declared: &dyn host::declared::Declared,
    ) -> Option<Self> {
        let parameters = header_parameters(&self.parameters)?;
        let defaults = self
            .defaults
            .iter()
            .map(|default| match default {
                Some(default) => default
                    .resolve(root, declared, &parameters, &self.path)
                    .ok()
                    .map(Some),
                None => Some(None),
            })
            .collect::<Option<Box<[_]>>>()?;
        Some(Self {
            path: self.path.clone(),
            identity: self.identity,
            parameters: self.parameters.clone(),
            defaults,
        })
    }
}

#[cfg(feature = "rust-import")]
impl ImplHeader {
    /// This header resolved against the declarations of the facade at
    /// `root`: `None` when its trait's arguments or self type name a type it
    /// lacks. A bound or projection that does is left out.
    pub(crate) fn resolve(
        &self,
        root: &host::path::Path,
        declared: &dyn host::declared::Declared,
    ) -> Option<Self> {
        let parameters = header_parameters(&self.parameters)?;
        let resolve = |types: &[host::type_ref::Type]| {
            resolve_header_types(types, root, declared, &parameters, root)
        };
        let (Some(arguments), Some(self_type)) = (
            resolve(&self.arguments),
            resolve(std::slice::from_ref(&self.self_type)),
        ) else {
            return None;
        };
        let bounds = self
            .bounds
            .iter()
            .filter_map(|bound| {
                Some(ImplBound {
                    ty: resolve(std::slice::from_ref(&bound.ty))?.into_vec().pop()?,
                    trait_identity: bound.trait_identity,
                    arguments: resolve(&bound.arguments)?,
                })
            })
            .collect::<Box<[_]>>();
        let one = |ty: &host::type_ref::Type| resolve(std::slice::from_ref(ty))?.into_vec().pop();
        let projections = self
            .projections
            .iter()
            .filter_map(|projection| {
                Some(ImplProjection {
                    ty: one(&projection.ty)?,
                    value: one(&projection.value)?,
                    cases: projection
                        .cases
                        .iter()
                        .map(|(this, value)| Some((one(this)?, one(value)?)))
                        .collect::<Option<_>>()?,
                })
            })
            .collect::<Box<[_]>>();
        Some(Self {
            parameters: self.parameters.clone(),
            trait_identity: self.trait_identity,
            arguments,
            self_type: self_type.into_vec().pop()?,
            bounds,
            projections,
        })
    }
}

/// `types` resolved against a facade's own types, with `parameters` as
/// type parameters: `None` when one names a type the facade lacks.
#[cfg(feature = "rust-import")]
fn resolve_header_types(
    types: &[host::type_ref::Type],
    root: &host::path::Path,
    declared: &dyn host::declared::Declared,
    parameters: &[(host::path::Path, crate::capability::Set)],
    item: &host::path::Path,
) -> Option<Box<[host::type_ref::Type]>> {
    types
        .iter()
        .map(|ty| ty.resolve(root, declared, parameters, item).ok())
        .collect()
}

#[cfg(feature = "rust-import")]
fn header_parameters(
    names: &[ImmutableString],
) -> Option<Vec<(host::path::Path, crate::capability::Set)>> {
    let mut parameters = names
        .iter()
        .map(|name| {
            Some((
                host::path::Path::parse(name.clone()).ok()?,
                crate::capability::Set::from_explicit([]),
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    parameters.sort_by(|(left, _), (right, _)| left.cmp(right));
    Some(parameters)
}

/// A header joins a facade's proof as a pseudo-module under a spelling
/// no module has: `prefix` and a digest of what it holds.
fn header_proof_path(
    root: &host::path::Path,
    prefix: &str,
    text: &str,
) -> Result<host::path::Path, host::Error> {
    use std::hash::{Hash as _, Hasher as _};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    host::path::Path::parse(format!("{root}::{prefix}_{:016x}", hasher.finish()))
}

pub(crate) struct RegistryParts {
    pub(crate) root: host::path::Path,
    pub(crate) modules: Box<[host::path::Path]>,
    pub(crate) types: Box<[host::type_def::Definition]>,
    pub(crate) items: Box<[host::item::Definition]>,
    pub(crate) aliases: Box<[(host::path::Path, host::path::Path)]>,
    pub(crate) reexports: Box<[(host::path::Path, host::path::Path)]>,
    pub(crate) type_aliases: Box<[TypeAlias]>,
    pub(crate) trait_headers: Box<[TraitHeader]>,
    pub(crate) impl_headers: Box<[ImplHeader]>,
    pub(crate) derives: Box<[host::path::Path]>,
    pub(crate) fingerprint: Fingerprint,
}

/// A deterministic exact proof of a complete static facade contract.
#[derive(Clone, Debug, Eq)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub struct Fingerprint {
    root: ImmutableString,
    abi: Abi,
    modules: Shared<[ImmutableString]>,
    types: Shared<[TypeProof]>,
    items: Shared<[host::item::Fingerprint]>,
    /// SHA-256 of the contract slices, computed whenever they are set, so
    /// hashing a proof that names this facade does not rewalk it.
    content: [u8; 32],
    /// Whether `content` is the digest a runner image carries for the
    /// facade (`host::image`), whose slices stay empty: the image builds
    /// the contract an entry at a time, and its generator digested it whole.
    carried: bool,
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

/// Opaque proof that one installed declaration belongs to an exact facade.
///
/// Dependencies are attached by [`host::Registry`](crate::host::Registry)
/// only after a complete facade has finished validation. Legacy direct
/// registrations deliberately have no dependency proof.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub struct Dependency {
    root: ImmutableString,
    fingerprint: Fingerprint,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(crate) struct TypeProof {
    contract: host::type_def::Contract,
    projections: Box<[host::type_def::Projection]>,
}

/// Collects and validates one facade before it can enter a registry.
#[derive(Debug)]
pub struct Builder {
    root: host::path::Path,
    abi: Abi,
    modules: Vec<host::path::Path>,
    types: Vec<host::type_def::Definition>,
    items: Vec<(host::path::Path, Vec<host::item::Definition>)>,
    projections: Vec<ProjectionDeclaration>,
    aliases: Vec<(host::path::Path, host::path::Path)>,
    reexports: Vec<(host::path::Path, host::path::Path)>,
    type_aliases: Vec<TypeAlias>,
    #[cfg(feature = "rust-import")]
    trait_headers: Vec<TraitHeader>,
    #[cfg(feature = "rust-import")]
    impl_headers: Vec<ImplHeader>,
    derives: Vec<host::path::Path>,
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
            modules: Vec::new(),
            types: Vec::new(),
            items: Vec::new(),
            projections: Vec::new(),
            aliases: Vec::new(),
            reexports: Vec::new(),
            type_aliases: Vec::new(),
            #[cfg(feature = "rust-import")]
            trait_headers: Vec::new(),
            #[cfg(feature = "rust-import")]
            impl_headers: Vec::new(),
            derives: Vec::new(),
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
            modules: Vec::new(),
            types: Vec::new(),
            items: Vec::new(),
            projections: Vec::new(),
            aliases: Vec::new(),
            reexports: Vec::new(),
            type_aliases: Vec::new(),
            #[cfg(feature = "rust-import")]
            trait_headers: Vec::new(),
            #[cfg(feature = "rust-import")]
            impl_headers: Vec::new(),
            derives: Vec::new(),
        })
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
        } else if self.alias_index(&path).is_ok() {
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

    /// Declare the module at `path` the one at `target`, both relative,
    /// as `pub use iced_wgpu as wgpu;` makes iced_renderer's `wgpu`: what
    /// lies below `path` is what lies below `target`, which holds it
    /// (`Registry::canonical`).
    #[cfg(all(test, feature = "rust-import"))]
    pub(crate) fn imported_reexport<A: host::ImportAllocation + ?Sized>(
        &mut self,
        path: &str,
        target: &str,
        allocation: &mut A,
    ) -> Result<&mut Self, host::Error> {
        match self.imported_module(path, allocation) {
            Ok(_)
            | Err(host::Error::DuplicateFacadeEntry {
                existing: "module", ..
            }) => {}
            Err(error) => return Err(error),
        }
        let path = host::path::Path::qualify_imported(
            &self.root,
            host::path::Path::parse_imported(path, allocation)?,
            allocation,
        )?;
        let target = host::path::Path::qualify_imported(
            &self.root,
            host::path::Path::parse_imported(target, allocation)?,
            allocation,
        )?;
        self.reexports
            .try_reserve(1)
            .map_err(|_| host::Error::Allocation)?;
        self.reexports.push((path, target));
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
        self.insert_alias(path, target)?;
        Ok(self)
    }

    /// Name a crate's type alias at `path`, as Rust's `pub type Result =
    /// std::result::Result<(), Error>;`: `parameters` are its type
    /// parameters, each with its default in `defaults` (empty when none has
    /// one), and `target` is the type it names. `path` is relative.
    pub(crate) fn type_alias(
        &mut self,
        path: &str,
        parameters: Box<[ImmutableString]>,
        defaults: Box<[Option<host::type_ref::Type>]>,
        target: host::type_ref::Type,
    ) -> Result<&mut Self, host::Error> {
        let path = host::path::Path::qualify(&self.root, &host::path::Path::parse(path)?);
        self.ensure_vacant(&path, "type alias")?;
        if self.alias_index(&path).is_ok()
            || self.type_aliases.iter().any(|alias| alias.path == path)
        {
            return Err(host::Error::DuplicateFacadeEntry {
                path: path.to_string().into(),
                existing: "re-export",
                incoming: "type alias",
            });
        }
        if !defaults.is_empty() && defaults.len() != parameters.len() {
            return Err(host::Error::InvalidTypeParameter(path.to_string().into()));
        }
        self.type_aliases
            .try_reserve(1)
            .map_err(|_| host::Error::Allocation)?;
        self.type_aliases.push(TypeAlias {
            path,
            parameters,
            defaults,
            target,
        });
        Ok(self)
    }

    /// Record a crate trait's header at `path`, relative (D604).
    #[cfg(feature = "rust-import")]
    pub(crate) fn trait_header(
        &mut self,
        path: &str,
        identity: [u8; 32],
        parameters: Box<[ImmutableString]>,
        defaults: Box<[Option<host::type_ref::Type>]>,
    ) -> Result<&mut Self, host::Error> {
        let path = host::path::Path::qualify(&self.root, &host::path::Path::parse(path)?);
        self.trait_headers
            .try_reserve(1)
            .map_err(|_| host::Error::Allocation)?;
        self.trait_headers.push(TraitHeader {
            path,
            identity,
            parameters,
            defaults,
        });
        Ok(self)
    }

    /// Record the crate's derive macro at `path`, relative, as serde's
    /// `Serialize`. A derive is in the macro namespace, so it shares its
    /// path with a trait or type of the same name.
    #[cfg(any(test, feature = "rust-import"))]
    pub(crate) fn derive(&mut self, path: &str) -> Result<&mut Self, host::Error> {
        let path = host::path::Path::qualify(&self.root, &host::path::Path::parse(path)?);
        self.derives
            .try_reserve(1)
            .map_err(|_| host::Error::Allocation)?;
        self.derives.push(path);
        Ok(self)
    }

    /// Record one impl of a crate trait (D604).
    #[cfg(feature = "rust-import")]
    pub(crate) fn impl_header(&mut self, header: ImplHeader) -> Result<&mut Self, host::Error> {
        self.impl_headers
            .try_reserve(1)
            .map_err(|_| host::Error::Allocation)?;
        self.impl_headers.push(header);
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
        self.insert_alias(path, target)?;
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
        self.insert_alias(path, target)?;
        Ok(self)
    }

    /// Re-export the std item `std` at `path`, as iced's `pub use
    /// crate::core::time::*` re-exports `core::time::Duration` at
    /// `time::Duration`: the path names that std item, which Rawr resolves by
    /// its std path. `path` is relative and `std` starts at `std`.
    pub(crate) fn std_alias(&mut self, path: &str, std: &str) -> Result<&mut Self, host::Error> {
        let path = host::path::Path::qualify(&self.root, &host::path::Path::parse(path)?);
        let target = host::path::Path::parse(std)?;
        if !host::is_std_alias_target(&target) {
            return Err(host::Error::DuplicateFacadeEntry {
                path: path.to_string().into(),
                existing: "re-export of an item outside std",
                incoming: "re-export",
            });
        }
        self.ensure_vacant(&path, "re-export")?;
        self.insert_alias(path, target)?;
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

    /// An imported type with no layout, as tests declare one.
    #[cfg(all(test, feature = "rust-import"))]
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
        self.imported_opaque_type(
            path,
            identity,
            implemented,
            parameters,
            defaults,
            Box::default(),
            shape,
            None,
            0,
            allocation,
        )
    }

    /// [`imported_type`](Self::imported_type) of an opaque `impl Trait`
    /// type, with the associated types it binds, or of a struct rustc lays
    /// out as `layout`, which a path may write with `dropped` type
    /// arguments it drops.
    #[cfg(feature = "rust-import")]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn imported_opaque_type(
        &mut self,
        path: &str,
        identity: host::type_def::import::Identity,
        implemented: &[crate::capability::Kind],
        parameters: Box<[ImmutableString]>,
        defaults: Box<[Option<host::type_ref::Type>]>,
        associated: Box<[host::type_def::Associated]>,
        shape: crate::Shared<host::type_def::Shape>,
        layout: Option<host::type_def::layout::Layout>,
        dropped: usize,
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
        )?
        .with_associated(associated)
        .with_layout(layout)
        .with_dropped(dropped);
        self.insert_type(definition)?;
        Ok(self)
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
        match self.item_index(&path) {
            Ok(index) => admit(&mut self.items[index].1, item, &path)?,
            Err(index) => {
                let mut declarations = Vec::new();
                admit(&mut declarations, item, &path)?;
                self.items
                    .try_reserve(1)
                    .map_err(|_| host::Error::Allocation)?;
                self.items.insert(index, (path, declarations));
            }
        }
        Ok(self)
    }

    /// Retain distinct imported methods until their resolved signatures can
    /// be partitioned from ordinary overloads. Ordinary facade builders still
    /// validate every declaration eagerly through `item`.
    #[cfg(feature = "rust-import")]
    pub(crate) fn imported_item(
        &mut self,
        item: host::item::Definition,
    ) -> Result<&mut Self, host::Error> {
        let path = host::path::Path::qualify(&self.root, item.path_ref());
        match self.item_index(&path) {
            Ok(index) => admit_imported(&mut self.items[index].1, item, &path)?,
            Err(index) => {
                let mut declarations = Vec::new();
                admit_imported(&mut declarations, item, &path)?;
                self.items
                    .try_reserve(1)
                    .map_err(|_| host::Error::Allocation)?;
                self.items.insert(index, (path, declarations));
            }
        }
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
        let declared = host::declared::Slices::new(&self.modules, &self.types, &[], &imported);

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
                resolved.push(item.resolve(&self.root, &declared)?);
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
        sort_items(&mut items)?;
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
                && !host::is_std_alias_target(target)
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
        let mut type_aliases = self
            .type_aliases
            .iter()
            .map(|alias| alias.resolve(&self.root, &declared))
            .collect::<Result<Vec<_>, _>>()?;
        type_aliases.sort_by(|left, right| left.path.cmp(&right.path));
        for alias in &type_aliases {
            modules.push(alias.proof_path()?);
        }
        // A derive joins the proof as a header does.
        for derive in &self.derives {
            modules.push(header_proof_path(
                &self.root,
                "derive",
                &derive.to_string(),
            )?);
        }
        let fingerprint = fingerprint(&self.root, self.abi, &modules, &self.types, &items)?;
        Ok(Definition(Shared::new(Body {
            root: self.root,
            abi: self.abi,
            modules: self.modules.into_boxed_slice(),
            types: self.types.into_boxed_slice(),
            items,
            aliases: self.aliases.into_boxed_slice(),
            reexports: self.reexports.into_boxed_slice(),
            type_aliases: type_aliases.into_boxed_slice(),
            trait_headers: Box::default(),
            impl_headers: Box::default(),
            derives: {
                let mut derives = self.derives;
                derives.sort();
                derives.dedup();
                derives.into_boxed_slice()
            },
            #[cfg(feature = "rust-import")]
            families: Box::default(),
            fingerprint,
            #[cfg(feature = "rust-import")]
            held: None,
        })))
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn finish_imported<A: host::ImportAllocation + ?Sized>(
        mut self,
        foreign: Box<[(host::type_def::import::Identity, host::type_def::Export)]>,
        allocation: &mut A,
    ) -> Result<(Definition, Vec<host::family::Pending>), host::Error> {
        validate_module_parents(&self.root, &self.modules)?;
        validate_type_parents(&self.root, &self.modules, &self.types)?;
        validate_type_identities(&self.types)?;
        resolve_projections(&mut self.types, self.projections)?;
        let imported =
            host::type_ref::ImportedTypes::from_imported_facade(&self.types, foreign, allocation)?;

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
        let alias_paths = self
            .type_aliases
            .iter()
            .map(|alias| alias.path.clone())
            .collect::<Vec<_>>();
        let declared =
            host::declared::Slices::new(&self.modules, &self.types, &alias_paths, &imported);
        let mut families = Vec::new();
        // Resolving one group temporarily owns its new storage while the
        // declaration and final ordinary-item buffers are still alive.
        let largest_group = self
            .items
            .iter()
            .map(|(_, items)| items.len())
            .max()
            .unwrap_or(0);
        allocation
            .retain_checked(
                Some(largest_group),
                largest_group.checked_mul(std::mem::size_of::<host::item::Definition>()),
            )
            .map_err(|_| host::Error::Allocation)?;
        for (_, declarations) in self.items {
            let mut resolved = Vec::new();
            resolved
                .try_reserve_exact(declarations.len())
                .map_err(|_| host::Error::Allocation)?;
            for item in declarations {
                resolved.push(item.resolve_imported(&self.root, &declared, allocation)?);
            }
            resolved.sort_by_key(host::item::Definition::fingerprint);
            match host::family::Pending::partition(resolved, allocation)? {
                host::family::Admission::Ordinary(resolved) => items.extend(resolved),
                host::family::Admission::Pending(family) => {
                    families
                        .try_reserve(1)
                        .map_err(|_| host::Error::Allocation)?;
                    families.push(family);
                }
            }
        }

        sort_items(&mut items)?;

        let (fingerprint_items, fingerprint_bytes) =
            fingerprint_usage(&self.root, &self.modules, &self.types, &items)?;
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
                && !host::is_std_alias_target(target)
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
        let mut type_aliases = Vec::new();
        type_aliases
            .try_reserve_exact(self.type_aliases.len())
            .map_err(|_| host::Error::Allocation)?;
        for alias in &self.type_aliases {
            type_aliases.push(alias.resolve(&self.root, &declared)?);
        }
        type_aliases.sort_by(|left, right| left.path.cmp(&right.path));
        let mut modules = Vec::new();
        modules
            .try_reserve_exact(self.modules.len() + self.aliases.len() + type_aliases.len())
            .map_err(|_| host::Error::Allocation)?;
        modules.extend(self.modules.iter().cloned());
        for (alias, target) in &self.aliases {
            // Re-exports join the proof under a spelling no module has.
            modules.push(host::path::Path::parse(format!("{alias}::{target}"))?);
        }
        for alias in &type_aliases {
            modules.push(alias.proof_path()?);
        }
        // Trait and impl headers (D604), resolved against the facade's own
        // types. A header naming a type the facade lacks is left out.
        let mut trait_headers = Vec::new();
        for header in &self.trait_headers {
            let Some(header) = header.resolve(&self.root, &declared) else {
                continue;
            };
            modules.push(header_proof_path(
                &self.root,
                "trait",
                &format!(
                    "{}|{:?}|{:?}|{:?}",
                    header.path, header.identity, header.parameters, header.defaults
                ),
            )?);
            trait_headers
                .try_reserve(1)
                .map_err(|_| host::Error::Allocation)?;
            trait_headers.push(header);
        }
        trait_headers.sort_by(|left, right| left.path.cmp(&right.path));
        let mut impl_headers = Vec::new();
        for header in &self.impl_headers {
            let Some(header) = header.resolve(&self.root, &declared) else {
                continue;
            };
            modules.push(header_proof_path(
                &self.root,
                "impl",
                &format!("{header:?}"),
            )?);
            impl_headers
                .try_reserve(1)
                .map_err(|_| host::Error::Allocation)?;
            impl_headers.push(header);
        }
        // A re-exported module joins the proof as a header does.
        self.reexports.sort();
        for (path, target) in &self.reexports {
            modules.push(header_proof_path(
                &self.root,
                "reexport",
                &format!("{path}|{target}"),
            )?);
        }
        // A derive joins the proof as a header does.
        for derive in &self.derives {
            modules.push(header_proof_path(
                &self.root,
                "derive",
                &derive.to_string(),
            )?);
        }
        let fingerprint = fingerprint(&self.root, self.abi, &modules, &self.types, &items)?;
        Ok((
            Definition(Shared::new(Body {
                root: self.root,
                abi: self.abi,
                modules: self.modules.into_boxed_slice(),
                types: self.types.into_boxed_slice(),
                items,
                aliases: self.aliases.into_boxed_slice(),
                reexports: self.reexports.into_boxed_slice(),
                type_aliases: type_aliases.into_boxed_slice(),
                trait_headers: trait_headers.into_boxed_slice(),
                impl_headers: impl_headers.into_boxed_slice(),
                derives: {
                    self.derives.sort();
                    self.derives.dedup();
                    self.derives.into_boxed_slice()
                },
                families: families.clone().into_boxed_slice(),
                fingerprint,
                #[cfg(feature = "rust-import")]
                held: None,
            })),
            families,
        ))
    }

    fn ensure_vacant(
        &self,
        path: &host::path::Path,
        incoming: &'static str,
    ) -> Result<(), host::Error> {
        // Items are values, in a namespace of their own (see `item`), so
        // only modules and types are taken in the type namespace.
        let existing = if self.modules.binary_search(path).is_ok() {
            Some("module")
        } else if self.type_index(path).is_ok() {
            Some("type")
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

    /// Where the re-export at `path` is, or would go: the builder keeps
    /// its re-exports in path order, so each is a search.
    fn alias_index(&self, path: &host::path::Path) -> Result<usize, usize> {
        self.aliases.binary_search_by(|(alias, _)| alias.cmp(path))
    }

    /// Re-export `target` at `path`, which no other re-export takes.
    fn insert_alias(
        &mut self,
        path: host::path::Path,
        target: host::path::Path,
    ) -> Result<(), host::Error> {
        let Err(index) = self.alias_index(&path) else {
            return Err(host::Error::DuplicateFacadeEntry {
                path: path.to_string().into(),
                existing: "re-export",
                incoming: "re-export",
            });
        };
        self.aliases
            .try_reserve(1)
            .map_err(|_| host::Error::Allocation)?;
        self.aliases.insert(index, (path, target));
        Ok(())
    }

    fn item_index(&self, path: &host::path::Path) -> Result<usize, usize> {
        // A plan's items come in path order, so each is at the end or
        // after it.
        match self.items.last().map(|(last, _)| last.cmp(path)) {
            None | Some(std::cmp::Ordering::Less) => Err(self.items.len()),
            Some(std::cmp::Ordering::Equal) => Ok(self.items.len() - 1),
            Some(std::cmp::Ordering::Greater) => self
                .items
                .binary_search_by(|(candidate, _)| candidate.cmp(path)),
        }
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

/// Add `item` to the declarations at `path`: the facade's other items
/// there.
pub(crate) fn admit(
    declarations: &mut Vec<host::item::Definition>,
    item: host::item::Definition,
    path: &host::path::Path,
) -> Result<(), host::Error> {
    // An item lives in the value namespace and a type in the type
    // namespace, so one path names both, as `lazy_static!` declares a
    // hidden `struct NAME` beside its `static NAME: NAME`. rustc has
    // already refused the crates where they clash (a unit or tuple
    // struct's constructor beside a value of its name).
    // A method whose impl fixes its receiver's type, as `and_then` of
    // `impl<T> Task<Option<T>>`, is a form the receiver picks. One that
    // the facade can't hold beside another form of the same name, as
    // one of another receiver mode or of the same inputs, gives way.
    #[cfg(feature = "rust-import")]
    {
        let conflicts = |existing: &host::item::Definition| -> Result<bool, host::Error> {
            Ok(same_namespace(existing, &item)
                && (!compatible_overload(existing, &item)
                    || existing.signature().same_inputs(item.signature())?))
        };
        if item.signature().instance().is_some() {
            for existing in declarations.iter() {
                if conflicts(existing)? {
                    return Ok(());
                }
            }
        } else {
            let mut kept = Vec::new();
            kept.try_reserve_exact(declarations.len())
                .map_err(|_| host::Error::Allocation)?;
            for existing in std::mem::take(declarations) {
                if existing.signature().instance().is_some() && conflicts(&existing)? {
                    continue;
                }
                kept.push(existing);
            }
            *declarations = kept;
        }
    }
    if let Some(existing) = declarations
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
    declarations
        .try_reserve(1)
        .map_err(|_| host::Error::Allocation)?;
    declarations.push(item);
    Ok(())
}

/// [`admit`] of an imported item: one of distinct imported methods at its
/// path joins them as it is, for their resolved signatures to partition.
#[cfg(feature = "rust-import")]
pub(crate) fn admit_imported(
    declarations: &mut Vec<host::item::Definition>,
    item: host::item::Definition,
    path: &host::path::Path,
) -> Result<(), host::Error> {
    if item.signature().instance().is_none()
        && declarations
            .iter()
            .all(|prior| prior.signature().instance().is_none())
        && declarations
            .iter()
            .any(|prior| host::family::distinct(prior, &item))
    {
        declarations
            .try_reserve(1)
            .map_err(|_| host::Error::Allocation)?;
        declarations.push(item);
        Ok(())
    } else {
        admit(declarations, item, path)
    }
}

#[cfg(feature = "rust-import")]
pub(crate) fn sort_items(items: &mut [host::item::Definition]) -> Result<(), host::Error> {
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
/// constructor may share its name with the shared reader of its field. An
/// operator a crate implements for a number, a function of the number and
/// the type, may share its name with the type's own operator methods.
pub(super) fn compatible_overload(
    existing: &host::item::Definition,
    item: &host::item::Definition,
) -> bool {
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
    // An operator of a crate's impl for a number, as `<f32 as Mul<Radians>>
    // ::mul`, beside the type's own `Mul` impls' `mul(self, ..)`.
    let operand = |item: &host::item::Definition| {
        constructor(item)
            && item
                .signature()
                .parameters()
                .first()
                .is_some_and(|first| matches!(first.shape(), host::type_ref::Shape::Builtin(_)))
    };
    existing.kind() == item.kind() && existing.receiver() == item.receiver()
        || builder(existing) && operand(item)
        || operand(existing) && builder(item)
        || value(existing) && function(item)
        || function(existing) && value(item)
        || reader(existing) && builder(item)
        || builder(existing) && reader(item)
        || constructor(existing) && reader(item)
        || reader(existing) && constructor(item)
}

impl Definition {
    /// The facade `held` in its runner's image, which builds each entry the
    /// first time a lookup reaches it.
    #[cfg(feature = "rust-import")]
    pub(crate) fn from_held(held: host::image::Held, type_aliases: Box<[TypeAlias]>) -> Self {
        Self(Shared::new(Body {
            root: held.root().clone(),
            abi: held.fingerprint().abi,
            modules: Box::default(),
            types: Box::default(),
            items: Box::default(),
            aliases: Box::default(),
            reexports: held.reexports().into(),
            type_aliases,
            trait_headers: Box::default(),
            impl_headers: Box::default(),
            derives: Box::default(),
            families: Box::default(),
            fingerprint: held.fingerprint().clone(),
            held: Some(held),
        }))
    }

    /// Return the script-visible root namespace.
    #[must_use]
    pub fn root(&self) -> &str {
        self.root.name()
    }

    /// Return this facade's declared ABI version.
    #[must_use]
    pub fn abi(&self) -> Abi {
        self.abi
    }

    /// Return the deterministic exact facade proof.
    #[must_use]
    pub fn fingerprint(&self) -> &Fingerprint {
        &self.fingerprint
    }

    /// Iterate over the facade's registered types in canonical path order.
    /// A facade held in its runner's image builds each of them.
    pub fn types(&self) -> Box<dyn ExactSizeIterator<Item = &host::type_def::Definition> + '_> {
        #[cfg(feature = "rust-import")]
        if let Some(held) = &self.held {
            return Box::new((0..held.type_count()).map(|index| held.type_at(index)));
        }
        Box::new(self.types.iter())
    }

    /// The facade's generic types of a linked native identity, which a
    /// generated runner attests one by one: a held facade's types are each
    /// an imported crate type, so it has none.
    #[doc(hidden)]
    pub fn native_generic_types(&self) -> impl Iterator<Item = &host::type_def::Definition> {
        self.types
            .iter()
            .filter(|ty| ty.identity().native().is_some() && !ty.parameters().is_empty())
    }

    /// Iterate over the facade's registered items in canonical path order.
    /// A facade held in its runner's image builds each of them.
    pub fn items(&self) -> Box<dyn Iterator<Item = &host::item::Definition> + '_> {
        #[cfg(feature = "rust-import")]
        if let Some(held) = &self.held {
            return Box::new(
                (0..held.group_count()).flat_map(|index| held.group_at(index).items()),
            );
        }
        Box::new(self.items.iter())
    }

    /// Iterate over the methods of the facade's families, which share a
    /// path and inputs and so are not among its items.
    #[cfg(feature = "rust-import")]
    pub(crate) fn family_methods(&self) -> Box<dyn Iterator<Item = &host::item::Definition> + '_> {
        if let Some(held) = &self.held {
            return Box::new(
                (0..held.group_count())
                    .filter_map(|index| held.group_at(index).family())
                    .flat_map(host::family::Pending::methods),
            );
        }
        Box::new(
            self.families
                .iter()
                .flat_map(host::family::Pending::methods),
        )
    }

    /// The facade's items and its families' methods of the imported
    /// selector `fingerprint`: the items in path order, then the methods.
    #[cfg(feature = "rust-import")]
    pub(crate) fn selected(&self, fingerprint: &[u8; 32]) -> Vec<&host::item::Definition> {
        if let Some(held) = &self.held {
            return held.selected(fingerprint);
        }
        self.items()
            .chain(self.family_methods())
            .filter(|item| {
                item.imported_identity()
                    .is_some_and(|identity| identity.fingerprint() == *fingerprint)
            })
            .collect()
    }

    /// The paths of the facade's types of the crate type `identity`, in
    /// path order.
    #[cfg(feature = "rust-import")]
    pub(crate) fn imported_paths(&self, identity: &[u8; 32]) -> Vec<ImmutableString> {
        if let Some(held) = &self.held {
            let mut indices =
                held.identity_types(&host::type_def::import::Identity::new(*identity));
            indices.sort_unstable();
            return indices
                .into_iter()
                .map(|index| held.type_path(index).to_string().into())
                .collect();
        }
        self.types()
            .filter(|ty| {
                ty.identity()
                    .imported()
                    .is_some_and(|imported| imported.as_bytes() == identity)
            })
            .map(|ty| ImmutableString::from(ty.path()))
            .collect()
    }

    /// The facade held in its runner's image, when it is one.
    #[cfg(feature = "rust-import")]
    pub(crate) fn held(&self) -> Option<&host::image::Held> {
        self.held.as_ref()
    }

    pub(crate) fn overloads(&self, path: &str) -> Option<&[host::item::Definition]> {
        let path = host::path::Path::parse(path).ok()?;
        #[cfg(feature = "rust-import")]
        if let Some(held) = &self.held {
            return held.items(&path);
        }
        item_group(&self.items, &path)
    }

    /// Export an opaque proof for a type declared by this finished facade.
    pub fn export_type(
        &self,
        path: impl Into<ImmutableString>,
    ) -> Result<host::type_def::Export, host::Error> {
        let path = host::path::Path::qualify(&self.root, &host::path::Path::parse(path)?);
        // Rawr exports only the types its own facades declare, as
        // `future::Future`.
        let definition = self
            .type_at(&path)
            .unwrap_or_else(|| panic!("bindings export only a type they declare, not `{path}`"));
        Ok(host::type_def::Export::new(
            self.root.clone(),
            self.fingerprint.clone(),
            definition.contract(),
        ))
    }

    /// Export the crate type of `identity` this finished facade declares,
    /// at its first path, as another facade's items name it.
    #[cfg(feature = "rust-import")]
    pub(crate) fn export_imported(
        &self,
        identity: &host::type_def::import::Identity,
    ) -> Option<host::type_def::Export> {
        if let Some(held) = &self.held {
            return held.export(identity);
        }
        let definition = self
            .types
            .iter()
            .find(|definition| definition.identity().imported() == Some(identity))?;
        Some(host::type_def::Export::new(
            self.root.clone(),
            self.fingerprint.clone(),
            definition.contract(),
        ))
    }

    pub(crate) fn root_path(&self) -> &host::path::Path {
        &self.root
    }

    /// The facade's type at the qualified `path`.
    pub(crate) fn type_at(&self, path: &host::path::Path) -> Option<&host::type_def::Definition> {
        #[cfg(feature = "rust-import")]
        if let Some(held) = &self.held {
            return held.type_index(path).map(|index| held.type_at(index));
        }
        self.types
            .binary_search_by(|definition| definition.path_ref().cmp(path))
            .ok()
            .map(|index| &self.types[index])
    }

    /// `path` below the module each re-exported module above it names, as
    /// the registry reads it (`host::through_reexports`).
    #[cfg(feature = "rust-import")]
    pub(crate) fn through_reexports(&self, path: &host::path::Path) -> Option<host::path::Path> {
        host::through_reexports(&self.reexports, path)
    }

    /// The parts the compiler's registry keeps: this contract's own when
    /// nothing else shares it, and else a copy.
    pub(crate) fn into_registry_parts(self) -> RegistryParts {
        let body = Shared::try_unwrap(self.0).unwrap_or_else(|shared| Body::clone(&shared));
        RegistryParts {
            root: body.root,
            modules: body.modules,
            types: body.types,
            items: body.items,
            aliases: body.aliases,
            reexports: body.reexports,
            type_aliases: body.type_aliases,
            trait_headers: body.trait_headers,
            impl_headers: body.impl_headers,
            derives: body.derives,
            fingerprint: body.fingerprint,
        }
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn registry_usage(&self) -> (Option<usize>, Option<usize>) {
        let item_count = self.items.len();
        let mut identities = self
            .types
            .iter()
            .filter_map(|definition| definition.identity().imported())
            .collect::<Vec<_>>();
        let imported_aliases = identities.len();
        identities.sort_unstable();
        identities.dedup();
        let imported_groups = identities.len();
        let items = Some(2_usize)
            .and_then(|items| items.checked_add(self.modules.len()))
            .and_then(|items| items.checked_add(self.types.len()))
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

    pub(crate) fn modules(&self) -> &[host::path::Path] {
        &self.modules
    }

    pub(crate) fn type_map(&self) -> &[host::type_def::Definition] {
        &self.types
    }

    /// How many items the facade holds apart from its runner's image.
    pub(crate) fn item_count(&self) -> usize {
        self.items.len()
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
        #[cfg(feature = "rust-import")]
        if let Some(held) = &self.held {
            return held.dependencies();
        }
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
        + shared_slice_overhead::<host::item::Fingerprint>();

    /// The bytes of this facade's contract. A type of another facade it
    /// names is that facade's export, which counts its fingerprint as a
    /// handle, so this walks this facade's own contract alone (#1309).
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
        let items = self.items.iter().try_fold(0_usize, |bytes, item| {
            bytes.checked_add(item.retained_bytes()?)
        })?;
        std::mem::size_of::<Self>()
            .checked_add(Self::COLLECTION_OVERHEAD)?
            .checked_add(self.root.len())?
            .checked_add(modules)?
            .checked_add(types)?
            .checked_add(items)
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn try_clone(&self) -> Result<Self, rawr_import::surface::Error> {
        // All collections belong to the finished immutable facade. Cloning
        // retains that authority; semantic normalization detaches before edits.
        Ok(self.clone())
    }
}

impl Fingerprint {
    /// The proof of the facade at `root` a runner image holds, which its
    /// generator digested as `content` (`host::image`).
    #[cfg(feature = "rust-import")]
    pub(crate) fn carried(root: &host::path::Path, abi: Abi, content: [u8; 32]) -> Self {
        Self {
            root: root.to_string().into(),
            abi,
            modules: Shared::from([]),
            types: Shared::from([]),
            items: Shared::from([]),
            content,
            carried: true,
        }
    }

    /// Recompute the content digest after the slices are set.
    fn seal(&mut self) {
        let mut content = ContentHasher(Sha256::new());
        self.modules.hash(&mut content);
        self.types.hash(&mut content);
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
        self.content == other.content && self.root == other.root && self.abi == other.abi
    }
}

impl Ord for Fingerprint {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Equality recognizes shared nested type proofs; ordering otherwise
        // walks their complete shapes even when the facade is unchanged.
        if self == other {
            return std::cmp::Ordering::Equal;
        }
        // A carried digest has no slices to walk: its content orders it.
        if self.carried || other.carried {
            return (&self.root, &self.abi, self.carried, &self.content).cmp(&(
                &other.root,
                &other.abi,
                other.carried,
                &other.content,
            ));
        }
        (
            &self.root,
            &self.abi,
            &self.modules,
            &self.types,
            &self.items,
        )
            .cmp(&(
                &other.root,
                &other.abi,
                &other.modules,
                &other.types,
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
        self.root.hash(state);
        self.abi.hash(state);
        self.content.hash(state);
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

impl crate::digest::Write for Fingerprint {
    fn write(&self, writer: &mut crate::digest::Writer) -> Result<(), crate::digest::Error> {
        use crate::digest::write_slice;

        writer.tag(0x2900);
        writer.tag(0x2901);
        writer.text(&self.root);
        writer.tag(0x2902);
        writer.u32(self.abi.get());
        // A carried digest stands for the contract its image holds.
        if self.carried {
            writer.tag(0x2908);
            writer.bytes(&self.content);
            return Ok(());
        }
        writer.tag(0x2903);
        writer.count(self.modules.len());
        for module in self.modules.iter() {
            writer.text(module);
        }
        writer.tag(0x2904);
        write_slice(writer, &self.types)?;
        writer.tag(0x2907);
        write_slice(writer, &self.items)?;
        Ok(())
    }
}

impl crate::digest::Write for Dependency {
    fn write(&self, writer: &mut crate::digest::Writer) -> Result<(), crate::digest::Error> {
        writer.tag(0x2a00);
        writer.tag(0x2a01);
        writer.text(&self.root);
        writer.tag(0x2a02);
        self.fingerprint.write(writer)?;
        Ok(())
    }
}

impl crate::digest::Write for TypeProof {
    fn write(&self, writer: &mut crate::digest::Writer) -> Result<(), crate::digest::Error> {
        use crate::digest::write_slice;

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
            .expect("qualified modules retain their root");
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
        let parent = path.parent().expect("qualified types retain their root");
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

/// Every type of one identity has one contract: the types of a facade
/// that share an identity, a type and its re-exports, agree.
///
/// The contract is an equality, so a type agrees with all of its identity
/// once it agrees with the first: each one is compared once, in identity
/// order, and the conflict named is the first in declaration order.
fn validate_type_identities(types: &[host::type_def::Definition]) -> Result<(), host::Error> {
    let mut order = Vec::new();
    order
        .try_reserve_exact(types.len())
        .map_err(|_| host::Error::Allocation)?;
    order.extend(0..types.len());
    // Stable, so each identity's types stay in declaration order.
    order.sort_by(|&left, &right| types[left].identity().cmp(types[right].identity()));
    let conflict = order
        .chunk_by(|&left, &right| types[left].identity() == types[right].identity())
        .filter_map(|group| {
            let (&first, rest) = group.split_first()?;
            let incoming = rest
                .iter()
                .copied()
                .find(|&index| !types[first].same_identity_contract(&types[index]))?;
            Some((first, incoming))
        })
        .min_by_key(|&(_, incoming)| incoming);
    match conflict {
        Some((existing, incoming)) => Err(host::Error::ConflictingHostIdentity {
            existing: types[existing].path().into(),
            incoming: types[incoming].path().into(),
        }),
        None => Ok(()),
    }
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

    #[test]
    fn fingerprint_order_matches_lexicographic_fields_with_shared_proofs() {
        fn original(left: &super::Fingerprint, right: &super::Fingerprint) -> std::cmp::Ordering {
            left.root
                .cmp(&right.root)
                .then_with(|| left.abi.cmp(&right.abi))
                .then_with(|| left.modules.cmp(&right.modules))
                .then_with(|| left.types.cmp(&right.types))
                .then_with(|| left.items.cmp(&right.items))
        }

        fn fixture(
            form: host::type_def::Form,
            leaf: host::type_ref::Builtin,
        ) -> super::Fingerprint {
            let mut builder = Builder::new("ordered", Abi::new(1).unwrap()).unwrap();
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
                        unsupported: None,
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
            builder.finish().unwrap().fingerprint().clone()
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
            (&deep.root, deep.abi, &deep.modules),
            (&base.root, base.abi, &base.modules)
        );
        assert_ne!(base, deep);
        assert_ne!(original(&base, &deep), std::cmp::Ordering::Equal);

        let mut variants = vec![base.clone(); 5];
        variants[0].root = "different".into();
        variants[1].abi = Abi::new(2).unwrap();
        variants[2].modules = Shared::default();
        variants[3].types =
            fixture(host::type_def::Form::Tuple, host::type_ref::Builtin::I64).types;
        variants[4].items =
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
        assert_eq!(variants.len(), 6);
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

    struct Marker;
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
    modules: &[host::path::Path],
    types: &[host::type_def::Definition],
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
        carried: false,
        root: root.to_string().into(),
        abi,
        modules: module_proofs.into_boxed_slice().into(),
        types: type_proofs.into_boxed_slice().into(),
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
    let items = items.iter().try_fold(0_usize, |bytes, item| {
        bytes.checked_add(item.fingerprint_retained_bytes()?)
    });
    let retained_bytes = std::mem::size_of::<Fingerprint>()
        .checked_add(Fingerprint::COLLECTION_OVERHEAD)
        .and_then(|bytes| bytes.checked_add(conversion_payload))
        .and_then(|bytes| bytes.checked_add(root.display_len()?))
        .and_then(|bytes| bytes.checked_add(modules?))
        .and_then(|bytes| bytes.checked_add(types?))
        .and_then(|bytes| bytes.checked_add(items?))
        .ok_or(host::Error::Allocation)?;
    Ok((retained_items, retained_bytes))
}
