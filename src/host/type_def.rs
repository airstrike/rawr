//! Static host type declarations.

use crate::{capability, host, ImmutableString, Shared};
use std::any::TypeId;
use std::collections::BTreeSet;
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

#[cfg(feature = "rust-import")]
/// Exact symbolic operation selected from an imported Rust contract.
pub type ImportedSelector = rawr_import::surface::identity::Selector;

#[cfg(not(feature = "rust-import"))]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// Uninhabited symbolic selector when Rust import support is disabled.
pub enum ImportedSelector {}

pub mod import;
pub mod native;

/// Stable compile identity for a linked or symbolic imported host type.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Identity {
    /// A native type whose runtime identity is linked into this process.
    Linked(native::Identity),
    /// A symbolic imported type with no runtime identity in this process.
    Imported(import::Identity),
}

/// Compiler metadata for one registered host type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Definition {
    path: host::path::Path,
    identity: Identity,
    parameters: Box<[ImmutableString]>,
    capabilities: capability::Set,
    storage: Storage,
    role: Role,
    usage: host::usage::Policy,
    visibility: Visibility,
    shape: Option<Shared<Shape>>,
    projections: Box<[Projection]>,
    exact_arity: bool,
}

/// The exact structural contract of one facade type.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Contract {
    path: host::path::Path,
    identity: Identity,
    parameters: Box<[ImmutableString]>,
    capabilities: capability::Set,
    storage: Storage,
    role: Role,
    usage: host::usage::Policy,
    visibility: Visibility,
    shape: Option<Shared<Shape>>,
}

/// Truthful compiler-visible structure for one host type.
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Shape {
    /// No field or variant operations are promised.
    #[default]
    Opaque,
    /// Exact source-visible struct fields.
    Struct {
        /// Exact Rust constructor grammar.
        form: Form,
        /// Exact imported constructor selector, when construction is proven.
        constructor: Option<ImportedSelector>,
        /// Exact source-visible fields in declaration order.
        fields: Box<[Field]>,
        /// Whether every field required to construct the value is visible.
        constructible: bool,
    },
    /// Exact source-visible enum variants.
    Enum(Box<[Variant]>),
    /// A trait script types may implement: its Rust declaration.
    Trait(ImmutableString),
}

/// Closed source constructor grammar for compiler-visible structure.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Form {
    /// A bare unit constructor.
    Unit,
    /// An ordered parenthesized constructor.
    Tuple,
    /// A named-field brace constructor.
    Record,
}

/// One readable imported struct or enum field.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Field {
    name: ImmutableString,
    ty: host::type_ref::Type,
    imported: ImportedSelector,
    #[cfg(feature = "rust-import")]
    imported_fingerprint: [u8; 32],
}

/// One source-visible imported enum variant.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Variant {
    name: ImmutableString,
    fields: Box<[Field]>,
    constructible: bool,
    imported: ImportedSelector,
    #[cfg(feature = "rust-import")]
    imported_fingerprint: [u8; 32],
}

/// Whether a facade type may be named by checked Rawr source.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Visibility {
    /// The type is part of the facade's source-visible contract.
    #[default]
    Public,
    /// The type is available only to compiler inference and host metadata.
    Private,
}

/// Where a facade-authenticated value may exist in checked source.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Storage {
    /// An ordinary owned value may follow its proven capabilities.
    #[default]
    Owned,
    /// A host-issued value may exist only in one checked invocation scope.
    Invocation,
}

/// Compiler-authenticated semantic role of one facade type.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Role {
    /// An ordinary facade-owned value type.
    #[default]
    Ordinary,
    /// The unique durable carrier for symbolic stream subscription plans.
    StreamPlan,
    /// A trait source names only as `impl Trait<..>`, which is its one
    /// projection's target: every implementor is that type (iced's `impl
    /// Widget<Message>` is an `Element<Message>`).
    Trait,
}

/// An opaque exported facade-type proof for authenticated dependencies.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Export {
    facade_root: host::path::Path,
    facade: host::facade::Fingerprint,
    contract: Contract,
}

/// One validated direct structural projection owned by its source type.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Projection {
    target: Target,
    arguments: Box<[Argument]>,
}

/// One exact target argument in a direct facade projection.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Argument {
    /// Substitute one named parameter from the source type.
    Source(ImmutableString),
    /// Infer a fresh target parameter independently at each projection site.
    Fresh,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum Target {
    Local(Contract),
    External(Export),
}

impl Definition {
    #[cfg(feature = "rust-import")]
    pub(crate) fn retained_bytes(&self) -> Option<usize> {
        let path = self
            .path
            .retained_bytes()?
            .checked_sub(std::mem::size_of::<host::path::Path>())?;
        let parameters = self.parameters.iter().try_fold(
            self.parameters
                .len()
                .checked_mul(std::mem::size_of::<ImmutableString>())?,
            |bytes, parameter| bytes.checked_add(parameter.len()),
        )?;
        let projections = self
            .projections
            .iter()
            .try_fold(0_usize, |bytes, projection| {
                bytes.checked_add(projection.retained_bytes()?)
            })?;
        std::mem::size_of::<Self>()
            .checked_add(path)?
            .checked_add(parameters)?
            .checked_add(projections)
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn fingerprint_retained_bytes(&self) -> Option<usize> {
        let contract = std::mem::size_of::<Contract>()
            .checked_add(
                self.path
                    .retained_bytes()?
                    .checked_sub(std::mem::size_of::<host::path::Path>())?,
            )?
            .checked_add(
                self.parameters.iter().try_fold(
                    self.parameters
                        .len()
                        .checked_mul(std::mem::size_of::<ImmutableString>())?,
                    |bytes, parameter| bytes.checked_add(parameter.len()),
                )?,
            )?;
        let projections = self
            .projections
            .iter()
            .try_fold(0_usize, |bytes, projection| {
                bytes.checked_add(projection.retained_bytes()?)
            })?;
        std::mem::size_of::<host::facade::TypeProof>()
            .checked_add(contract.checked_sub(std::mem::size_of::<Contract>())?)?
            .checked_add(projections)
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn try_clone(&self) -> Result<Self, rawr_import::surface::Error> {
        let mut parameters = Vec::new();
        parameters
            .try_reserve_exact(self.parameters.len())
            .map_err(|_| rawr_import::surface::Error::Allocation)?;
        parameters.extend(self.parameters.iter().cloned());
        let mut projections = Vec::new();
        projections
            .try_reserve_exact(self.projections.len())
            .map_err(|_| rawr_import::surface::Error::Allocation)?;
        for projection in &self.projections {
            projections.push(projection.try_clone()?);
        }
        Ok(Self {
            path: self.path.try_clone()?,
            identity: self.identity.clone(),
            parameters: parameters.into_boxed_slice(),
            capabilities: self.capabilities,
            storage: self.storage,
            role: self.role,
            usage: self.usage,
            visibility: self.visibility,
            shape: self.shape.clone(),
            projections: projections.into_boxed_slice(),
            exact_arity: self.exact_arity,
        })
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn try_clone_without_projections(
        &self,
    ) -> Result<Self, rawr_import::surface::Error> {
        let mut definition = self.try_clone()?;
        definition.projections = Box::default();
        Ok(definition)
    }

    pub(crate) fn declared<T: 'static>(
        path: host::path::Path,
        parameters: &[&str],
        capabilities: &[capability::Kind],
        storage: Storage,
        role: Role,
        usage: host::usage::Policy,
        visibility: Visibility,
    ) -> Result<Self, host::Error> {
        let mut names = BTreeSet::new();
        let mut resolved = Vec::with_capacity(parameters.len());
        for parameter in parameters {
            let parameter_path = host::path::Path::parse(*parameter)?;
            if parameter_path.segments().len() != 1 {
                return Err(host::Error::InvalidTypeParameter((*parameter).into()));
            }
            if !names.insert(parameter_path.clone()) {
                return Err(host::Error::DuplicateTypeParameter {
                    item: path.to_string().into(),
                    parameter: (*parameter).into(),
                });
            }
            resolved.push((*parameter).into());
        }
        let capabilities = capability::Set::from_explicit(capabilities.iter().copied());
        validate_capabilities(&path.to_string(), capabilities)?;
        Ok(Self {
            path,
            identity: Identity::Linked(native::Identity::of::<T>()),
            parameters: resolved.into_boxed_slice(),
            capabilities,
            storage,
            role,
            usage,
            visibility,
            shape: None,
            projections: Box::default(),
            exact_arity: true,
        })
    }

    pub(crate) fn legacy(path: host::path::Path, identity: TypeId) -> Self {
        Self {
            path,
            identity: Identity::Linked(native::Identity::from_type_id(identity)),
            parameters: Box::default(),
            capabilities: capability::Set::new(),
            storage: Storage::Owned,
            role: Role::Ordinary,
            usage: host::usage::Policy::Ordinary,
            visibility: Visibility::Public,
            shape: None,
            projections: Box::default(),
            exact_arity: false,
        }
    }

    /// Return this type's canonical qualified facade path.
    #[must_use]
    pub fn path(&self) -> String {
        self.path.to_string()
    }

    pub(crate) const fn path_ref(&self) -> &host::path::Path {
        &self.path
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn imported(
        path: host::path::Path,
        identity: import::Identity,
        copy: bool,
        default: bool,
        debug: bool,
        parameters: Box<[ImmutableString]>,
        shape: Shared<Shape>,
    ) -> Result<Self, host::Error> {
        validate_parameters(&path, &parameters)?;
        Ok(Self {
            path,
            identity: Identity::Imported(identity),
            parameters,
            capabilities: capability::Set::from_explicit(
                copy.then_some([capability::Kind::Clone, capability::Kind::Copy])
                    .into_iter()
                    .flatten()
                    .chain(default.then_some(capability::Kind::Default))
                    .chain(debug.then_some(capability::Kind::Debug)),
            ),
            storage: Storage::Owned,
            role: Role::Ordinary,
            usage: host::usage::Policy::Ordinary,
            visibility: Visibility::Public,
            shape: (!matches!(shape.as_ref(), Shape::Opaque)).then_some(shape),
            projections: Box::default(),
            exact_arity: true,
        })
    }

    /// Return the stable compile identity attached to this host type.
    #[must_use]
    pub const fn identity(&self) -> &Identity {
        &self.identity
    }

    /// Return native runtime evidence only for a linked host type.
    #[must_use]
    pub const fn native_identity(&self) -> Option<TypeId> {
        self.identity.native()
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn imported_declaration(&self) -> Option<ImportedSelector> {
        self.identity
            .imported()
            .map(|_| ImportedSelector::Declaration)
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn imported_constructor(&self) -> Option<&ImportedSelector> {
        match self.shape() {
            Shape::Struct { constructor, .. } => constructor.as_ref(),
            Shape::Opaque | Shape::Enum(_) | Shape::Trait(_) => None,
        }
    }

    /// Return the declared structural type parameters in order.
    #[must_use]
    pub const fn parameters(&self) -> &[ImmutableString] {
        &self.parameters
    }

    /// Return the capabilities explicitly promised by the host.
    #[must_use]
    pub const fn capabilities(&self) -> capability::Set {
        self.capabilities
    }

    /// Return the checked source storage class promised by the facade.
    #[must_use]
    pub const fn storage(&self) -> Storage {
        self.storage
    }

    /// Return the compiler-authenticated semantic role of this facade type.
    #[must_use]
    pub const fn role(&self) -> Role {
        self.role
    }

    /// Return the value's discarded-expression policy.
    #[must_use]
    pub const fn usage(&self) -> host::usage::Policy {
        self.usage
    }

    /// Return whether checked source may name this facade type.
    #[must_use]
    pub const fn visibility(&self) -> Visibility {
        self.visibility
    }

    /// Return exact imported structure, or opaque linked structure.
    #[must_use]
    pub fn shape(&self) -> &Shape {
        self.shape.as_deref().unwrap_or(&Shape::Opaque)
    }

    /// Return the direct structural projections declared by this type's facade.
    #[must_use]
    pub const fn projections(&self) -> &[Projection] {
        &self.projections
    }

    pub(crate) fn with_shape(mut self, shape: Shape) -> Self {
        self.shape = (!matches!(shape, Shape::Opaque)).then(|| shape.into());
        self
    }

    pub(crate) const fn exact_arity(&self) -> bool {
        self.exact_arity
    }

    pub(crate) fn same_identity_contract(&self, other: &Self) -> bool {
        self.identity == other.identity
            && self.parameters == other.parameters
            && self.capabilities == other.capabilities
            && self.storage == other.storage
            && self.role == other.role
            && self.usage == other.usage
            && self.visibility == other.visibility
            && self.shape == other.shape
            && self.projections == other.projections
            && self.exact_arity == other.exact_arity
    }

    pub(crate) fn contract(&self) -> Contract {
        Contract {
            path: self.path.clone(),
            identity: self.identity.clone(),
            parameters: self.parameters.clone(),
            capabilities: self.capabilities,
            storage: self.storage,
            role: self.role,
            usage: self.usage,
            visibility: self.visibility,
            shape: self.shape.clone(),
        }
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn try_contract(&self) -> Result<Contract, rawr_import::surface::Error> {
        let mut parameters = Vec::new();
        parameters
            .try_reserve_exact(self.parameters.len())
            .map_err(|_| rawr_import::surface::Error::Allocation)?;
        parameters.extend(self.parameters.iter().cloned());
        Ok(Contract {
            path: self.path.try_clone()?,
            identity: self.identity.clone(),
            parameters: parameters.into_boxed_slice(),
            capabilities: self.capabilities,
            storage: self.storage,
            role: self.role,
            usage: self.usage,
            visibility: self.visibility,
            shape: self.shape.clone(),
        })
    }

    pub(crate) fn set_projections(&mut self, projections: Box<[Projection]>) {
        self.projections = projections;
    }
}

impl Contract {
    #[cfg(feature = "rust-import")]
    pub(crate) fn retained_bytes(&self) -> Option<usize> {
        let path = self
            .path
            .retained_bytes()?
            .checked_sub(std::mem::size_of::<host::path::Path>())?;
        let parameters = self.parameters.iter().try_fold(
            self.parameters
                .len()
                .checked_mul(std::mem::size_of::<ImmutableString>())?,
            |bytes, parameter| bytes.checked_add(parameter.len()),
        )?;
        std::mem::size_of::<Self>()
            .checked_add(path)?
            .checked_add(parameters)
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn try_clone(&self) -> Result<Self, rawr_import::surface::Error> {
        let mut parameters = Vec::new();
        parameters
            .try_reserve_exact(self.parameters.len())
            .map_err(|_| rawr_import::surface::Error::Allocation)?;
        parameters.extend(self.parameters.iter().cloned());
        Ok(Self {
            path: self.path.try_clone()?,
            identity: self.identity.clone(),
            parameters: parameters.into_boxed_slice(),
            capabilities: self.capabilities,
            storage: self.storage,
            role: self.role,
            usage: self.usage,
            visibility: self.visibility,
            shape: self.shape.clone(),
        })
    }

    /// Return the target type's canonical qualified path.
    #[must_use]
    pub fn path(&self) -> String {
        self.path.to_string()
    }

    /// Return the target type's structural parameter names.
    #[must_use]
    pub const fn parameters(&self) -> &[ImmutableString] {
        &self.parameters
    }

    /// Return the capabilities explicitly promised by the target facade type.
    #[must_use]
    pub const fn capabilities(&self) -> capability::Set {
        self.capabilities
    }

    /// Return the exported type's checked source storage class.
    #[must_use]
    pub const fn storage(&self) -> Storage {
        self.storage
    }

    /// Return the compiler-authenticated semantic role of this type contract.
    #[must_use]
    pub const fn role(&self) -> Role {
        self.role
    }

    /// Return the exported type's discarded-expression policy.
    #[must_use]
    pub const fn usage(&self) -> host::usage::Policy {
        self.usage
    }

    /// Return whether checked source may name this facade type.
    #[must_use]
    pub const fn visibility(&self) -> Visibility {
        self.visibility
    }

    /// Return exact imported structure, or opaque linked structure.
    #[must_use]
    pub fn shape(&self) -> &Shape {
        self.shape.as_deref().unwrap_or(&Shape::Opaque)
    }

    pub(crate) const fn path_ref(&self) -> &host::path::Path {
        &self.path
    }

    pub(crate) const fn runtime_identity(&self) -> Option<TypeId> {
        self.identity.native()
    }

    #[cfg(feature = "rust-import")]
    pub(crate) const fn identity_ref(&self) -> &Identity {
        &self.identity
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn normalize_profile(
        &mut self,
        profile: &host::facade::ProfileFinalization<'_>,
    ) -> usize {
        let mut completed = usize::from(profile.normalize_identity(&self.path, &mut self.identity));
        if let Some(shape) = &mut self.shape {
            completed =
                completed.saturating_add(Shared::make_mut(shape).normalize_profile(profile));
        }
        completed
    }
}

impl Shape {
    #[cfg(feature = "rust-import")]
    pub(crate) fn normalize_profile(
        &mut self,
        profile: &host::facade::ProfileFinalization<'_>,
    ) -> usize {
        match self {
            Self::Opaque | Self::Trait(_) => 0,
            Self::Struct { fields, .. } => fields.iter_mut().fold(0_usize, |completed, field| {
                completed.saturating_add(field.ty.normalize_profile(profile))
            }),
            Self::Enum(variants) => variants.iter_mut().fold(0_usize, |completed, variant| {
                completed.saturating_add(variant.fields.iter_mut().fold(
                    0_usize,
                    |completed, field| {
                        completed.saturating_add(field.ty.normalize_profile(profile))
                    },
                ))
            }),
        }
    }
}

impl Export {
    #[cfg(feature = "rust-import")]
    pub(crate) fn normalize_profile(
        &mut self,
        profile: &host::facade::ProfileFinalization<'_>,
    ) -> usize {
        profile
            .normalize_fingerprint(&mut self.facade)
            .saturating_add(self.contract.normalize_profile(profile))
    }
}

impl Projection {
    #[cfg(feature = "rust-import")]
    pub(crate) fn normalize_profile(
        &mut self,
        profile: &host::facade::ProfileFinalization<'_>,
    ) -> usize {
        match &mut self.target {
            Target::Local(contract) => contract.normalize_profile(profile),
            Target::External(export) => export.normalize_profile(profile),
        }
    }
}

impl Export {
    #[cfg(feature = "rust-import")]
    pub(crate) fn retained_bytes(&self) -> Option<usize> {
        let root = self
            .facade_root
            .retained_bytes()?
            .checked_sub(std::mem::size_of::<host::path::Path>())?;
        let facade = self
            .facade
            .retained_bytes()?
            .checked_sub(std::mem::size_of::<host::facade::Fingerprint>())?;
        let contract = self
            .contract
            .retained_bytes()?
            .checked_sub(std::mem::size_of::<Contract>())?;
        std::mem::size_of::<Self>()
            .checked_add(root)?
            .checked_add(facade)?
            .checked_add(contract)
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn try_clone(&self) -> Result<Self, rawr_import::surface::Error> {
        Ok(Self {
            facade_root: self.facade_root.try_clone()?,
            facade: self.facade.try_clone()?,
            contract: self.contract.try_clone()?,
        })
    }

    pub(crate) fn new(
        facade_root: host::path::Path,
        facade: host::facade::Fingerprint,
        contract: Contract,
    ) -> Self {
        Self {
            facade_root,
            facade,
            contract,
        }
    }

    /// Return the exported type's canonical qualified path.
    #[must_use]
    pub fn path(&self) -> String {
        self.contract.path()
    }

    /// Return the exported type's structural parameter names.
    #[must_use]
    pub const fn parameters(&self) -> &[ImmutableString] {
        self.contract.parameters()
    }

    /// Return the exported type's checked source storage class.
    #[must_use]
    pub const fn storage(&self) -> Storage {
        self.contract.storage()
    }

    pub(crate) const fn facade_root(&self) -> &host::path::Path {
        &self.facade_root
    }

    pub(crate) const fn facade(&self) -> &host::facade::Fingerprint {
        &self.facade
    }

    pub(crate) const fn contract(&self) -> &Contract {
        &self.contract
    }

    pub(crate) fn dependency(&self) -> host::facade::Dependency {
        host::facade::Dependency::new(&self.facade_root, self.facade.clone())
    }
}

impl Projection {
    #[cfg(feature = "rust-import")]
    pub(crate) fn retained_bytes(&self) -> Option<usize> {
        let target = match &self.target {
            Target::Local(contract) => contract
                .retained_bytes()?
                .checked_sub(std::mem::size_of::<Contract>())?,
            Target::External(export) => export
                .retained_bytes()?
                .checked_sub(std::mem::size_of::<Export>())?,
        };
        let arguments = self.arguments.iter().try_fold(
            self.arguments
                .len()
                .checked_mul(std::mem::size_of::<Argument>())?,
            |bytes, argument| match argument {
                Argument::Source(name) => bytes.checked_add(name.len()),
                Argument::Fresh => Some(bytes),
            },
        )?;
        std::mem::size_of::<Self>()
            .checked_add(target)?
            .checked_add(arguments)
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn try_clone(&self) -> Result<Self, rawr_import::surface::Error> {
        let target = match &self.target {
            Target::Local(contract) => Target::Local(contract.try_clone()?),
            Target::External(export) => Target::External(export.try_clone()?),
        };
        let mut arguments = Vec::new();
        arguments
            .try_reserve_exact(self.arguments.len())
            .map_err(|_| rawr_import::surface::Error::Allocation)?;
        arguments.extend(self.arguments.iter().cloned());
        Ok(Self {
            target,
            arguments: arguments.into_boxed_slice(),
        })
    }

    pub(crate) fn local(target: Contract, arguments: Box<[Argument]>) -> Self {
        Self {
            target: Target::Local(target),
            arguments,
        }
    }

    pub(crate) fn external(target: Export, arguments: Box<[Argument]>) -> Self {
        Self {
            target: Target::External(target),
            arguments,
        }
    }

    /// Return the canonical target type path.
    #[must_use]
    pub fn target(&self) -> String {
        self.target_contract().path()
    }

    /// Return the exact target-argument substitutions.
    #[must_use]
    pub const fn arguments(&self) -> &[Argument] {
        &self.arguments
    }

    pub(crate) const fn target_contract(&self) -> &Contract {
        match &self.target {
            Target::Local(contract) => contract,
            Target::External(export) => export.contract(),
        }
    }

    pub(crate) const fn external_target(&self) -> Option<&Export> {
        match &self.target {
            Target::Local(_) => None,
            Target::External(export) => Some(export),
        }
    }

    pub(crate) fn target_dependency(&self) -> Option<host::facade::Dependency> {
        self.external_target().map(Export::dependency)
    }
}

impl Argument {
    /// Name one source parameter to substitute into the target.
    pub fn source(name: impl Into<ImmutableString>) -> Result<Self, host::Error> {
        let name = name.into();
        let path = host::path::Path::parse(name.clone())?;
        if path.segments().len() != 1 {
            return Err(host::Error::InvalidTypeParameter(name));
        }
        Ok(Self::Source(name))
    }

    /// Request one fresh target parameter at each projection use site.
    #[must_use]
    pub const fn fresh() -> Self {
        Self::Fresh
    }
}

impl Identity {
    #[cfg(any(test, feature = "rust-import"))]
    pub(crate) const fn linked(identity: TypeId) -> Self {
        Self::Linked(native::Identity::from_type_id(identity))
    }

    /// Borrow symbolic imported identity when this type is not linked.
    #[must_use]
    pub const fn imported(&self) -> Option<&import::Identity> {
        match self {
            Self::Linked(_) => None,
            Self::Imported(identity) => Some(identity),
        }
    }

    /// Return native runtime evidence only for a linked identity.
    #[must_use]
    pub const fn native(&self) -> Option<TypeId> {
        match self {
            Self::Linked(identity) => Some(identity.type_id()),
            Self::Imported(_) => None,
        }
    }
}

// Selector::Hash already writes its canonical SHA-256 fingerprint. Retain that
// exact value with the immutable selector instead of recomputing it on every
// traversal of a shared facade. All other fields keep their original hash order.
impl std::hash::Hash for Field {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::hash::Hash::hash(&self.name, state);
        std::hash::Hash::hash(&self.ty, state);
        #[cfg(feature = "rust-import")]
        std::hash::Hash::hash(&self.imported_fingerprint, state);
        #[cfg(not(feature = "rust-import"))]
        std::hash::Hash::hash(&self.imported, state);
    }
}

impl std::hash::Hash for Variant {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::hash::Hash::hash(&self.name, state);
        std::hash::Hash::hash(&self.fields, state);
        std::hash::Hash::hash(&self.constructible, state);
        #[cfg(feature = "rust-import")]
        std::hash::Hash::hash(&self.imported_fingerprint, state);
        #[cfg(not(feature = "rust-import"))]
        std::hash::Hash::hash(&self.imported, state);
    }
}

impl Field {
    #[cfg(feature = "rust-import")]
    pub(crate) fn new(
        name: ImmutableString,
        ty: host::type_ref::Type,
        imported: rawr_import::surface::identity::Selector,
    ) -> Self {
        let imported_fingerprint = imported.fingerprint();
        Self {
            name,
            ty,
            imported,
            imported_fingerprint,
        }
    }

    /// Return the unchanged Rust field name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Return the exact symbolic field type.
    #[must_use]
    pub const fn ty(&self) -> &host::type_ref::Type {
        &self.ty
    }

    #[cfg(feature = "rust-import")]
    pub(crate) const fn imported_identity(&self) -> &ImportedSelector {
        &self.imported
    }
}

impl Variant {
    #[cfg(feature = "rust-import")]
    pub(crate) fn new(
        name: ImmutableString,
        fields: Box<[Field]>,
        constructible: bool,
        imported: rawr_import::surface::identity::Selector,
    ) -> Self {
        let imported_fingerprint = imported.fingerprint();
        Self {
            name,
            fields,
            constructible,
            imported,
            imported_fingerprint,
        }
    }

    /// Return the unchanged Rust variant name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Return exact source-visible variant fields.
    #[must_use]
    pub const fn fields(&self) -> &[Field] {
        &self.fields
    }

    /// Return whether every field required for construction is visible.
    #[must_use]
    pub const fn constructible(&self) -> bool {
        self.constructible
    }

    #[cfg(feature = "rust-import")]
    pub(crate) const fn imported_identity(&self) -> &ImportedSelector {
        &self.imported
    }
}

impl crate::preview::digest::Write for Contract {
    fn write(
        &self,
        writer: &mut crate::preview::digest::Writer,
    ) -> Result<(), crate::preview::digest::Error> {
        writer.tag(0x2600);
        writer.tag(0x2601);
        writer.text(&self.path.to_string());
        writer.tag(0x2602);
        writer.count(self.parameters.len());
        for parameter in &self.parameters {
            writer.text(parameter);
        }
        writer.tag(0x2603);
        self.capabilities.write(writer)?;
        writer.tag(0x2604);
        writer.bool(matches!(self.storage, Storage::Invocation));
        writer.tag(0x2605);
        writer.bool(matches!(self.usage, host::usage::Policy::Required));
        writer.tag(0x2606);
        writer.tag(match self.role {
            Role::Ordinary => 0x2611,
            Role::StreamPlan => 0x2612,
            Role::Trait => 0x2613,
        });
        writer.tag(0x2607);
        writer.bool(matches!(self.visibility, Visibility::Private));
        if let Identity::Imported(identity) = &self.identity {
            writer.tag(0x2608);
            writer.bytes(identity.as_bytes());
        }
        writer.tag(0x2609);
        match self.shape() {
            Shape::Opaque => writer.tag(0x2621),
            Shape::Struct {
                form,
                fields,
                constructible,
                ..
            } => {
                writer.tag(0x2622);
                writer.tag(match form {
                    Form::Unit => 0x2631,
                    Form::Tuple => 0x2632,
                    Form::Record => 0x2633,
                });
                writer.bool(*constructible);
                writer.count(fields.len());
                for field in fields {
                    writer.text(field.name());
                    field.ty().write(writer)?;
                }
            }
            Shape::Trait(declaration) => {
                writer.tag(0x2624);
                writer.text(declaration);
            }
            Shape::Enum(variants) => {
                writer.tag(0x2623);
                writer.count(variants.len());
                for variant in variants {
                    writer.text(variant.name());
                    writer.bool(variant.constructible());
                    writer.count(variant.fields().len());
                    for field in variant.fields() {
                        writer.text(field.name());
                        field.ty().write(writer)?;
                    }
                }
            }
        }
        Ok(())
    }
}

impl crate::preview::digest::Write for Export {
    fn write(
        &self,
        writer: &mut crate::preview::digest::Writer,
    ) -> Result<(), crate::preview::digest::Error> {
        writer.tag(0x2700);
        writer.tag(0x2701);
        writer.text(&self.facade_root.to_string());
        writer.tag(0x2702);
        self.facade.write(writer)?;
        writer.tag(0x2703);
        self.contract.write(writer)?;
        Ok(())
    }
}

impl crate::preview::digest::Write for Projection {
    fn write(
        &self,
        writer: &mut crate::preview::digest::Writer,
    ) -> Result<(), crate::preview::digest::Error> {
        writer.tag(0x2800);
        writer.tag(0x2801);
        match &self.target {
            Target::Local(contract) => {
                writer.tag(0x2811);
                contract.write(writer)?;
            }
            Target::External(export) => {
                writer.tag(0x2812);
                export.write(writer)?;
            }
        }
        writer.tag(0x2802);
        writer.count(self.arguments.len());
        for argument in &self.arguments {
            match argument {
                Argument::Source(parameter) => {
                    writer.tag(0x2821);
                    writer.text(parameter);
                }
                Argument::Fresh => writer.tag(0x2822),
            }
        }
        Ok(())
    }
}

pub(crate) fn validate_capabilities(
    declaration: &str,
    capabilities: capability::Set,
) -> Result<(), host::Error> {
    for (capability, required) in [
        (capability::Kind::Copy, capability::Kind::Clone),
        (capability::Kind::Eq, capability::Kind::PartialEq),
    ] {
        if capabilities.contains(capability) && !capabilities.contains(required) {
            return Err(host::Error::InvalidCapabilitySet {
                declaration: declaration.into(),
                capability: capability.name(),
                required: required.name(),
            });
        }
    }
    Ok(())
}

#[cfg(feature = "rust-import")]
fn validate_parameters(
    path: &host::path::Path,
    parameters: &[ImmutableString],
) -> Result<(), host::Error> {
    let mut names = Vec::new();
    names
        .try_reserve_exact(parameters.len())
        .map_err(|_| host::Error::Allocation)?;
    for parameter in parameters {
        let parameter_path = host::path::Path::parse(parameter.clone())?;
        if parameter_path.segments().len() != 1 {
            return Err(host::Error::InvalidTypeParameter(parameter.clone()));
        }
        match names.binary_search(&parameter_path) {
            Ok(_) => {
                return Err(host::Error::DuplicateTypeParameter {
                    item: path.to_string().into(),
                    parameter: parameter.clone(),
                });
            }
            Err(index) => names.insert(index, parameter_path),
        }
    }
    Ok(())
}

#[cfg(test)]
mod layout_tests {
    use super::{Contract, Definition, Shape};
    use crate::Shared;
    use std::mem::size_of;

    #[test]
    fn imported_shape_metadata_stays_compact() {
        assert_eq!(size_of::<Option<Shared<Shape>>>(), size_of::<usize>());
        assert_eq!(size_of::<Definition>(), 104);
        assert!(size_of::<Contract>() <= 88);
    }
}

#[cfg(all(test, feature = "rust-import"))]
mod selector_hash_tests {
    use super::{Field, Variant};
    use crate::host;
    use crate::host::type_ref::{Builtin, Type};
    use rawr_import::surface::identity::{Form, Selector};
    use std::hash::{Hash, Hasher};

    #[derive(Default)]
    struct Writes(Vec<Vec<u8>>);

    impl Hasher for Writes {
        fn finish(&self) -> u64 {
            0
        }
        fn write(&mut self, bytes: &[u8]) {
            self.0.push(bytes.to_vec());
        }
    }

    fn writes(value: &impl Hash) -> Vec<Vec<u8>> {
        let mut output = Writes::default();
        value.hash(&mut output);
        output.0
    }

    #[derive(Hash)]
    struct OriginalField<'a> {
        name: &'a crate::ImmutableString,
        ty: &'a Type,
        imported: &'a Selector,
    }

    #[derive(Hash)]
    struct OriginalVariant<'a> {
        name: &'a crate::ImmutableString,
        fields: &'a [Field],
        constructible: bool,
        imported: &'a Selector,
    }

    #[test]
    fn retained_selector_hash_preserves_original_writes_and_order() {
        let named: rawr_import::surface::identity::Field =
            serde_json::from_str(r#"{"index":3,"name":"named🦀"}"#).expect("field fixture");
        let selectors = [
            Selector::StructField(named.clone()),
            Selector::StructConstructor(Form::Named {
                fields: Box::from([named.clone()]),
            }),
            Selector::EnumVariant {
                index: 2,
                name: "Named".into(),
                form: Form::Named {
                    fields: Box::from([named]),
                },
            },
            Selector::Declaration,
            Selector::Unavailable,
            Selector::StructConstructor(Form::Unit),
            Selector::StructConstructor(Form::Tuple { arity: 2 }),
            Selector::EnumVariant {
                index: 0,
                name: "First".into(),
                form: Form::Unit,
            },
            Selector::EnumVariant {
                index: 7,
                name: "Other🦀".into(),
                form: Form::Tuple { arity: 1 },
            },
        ];
        let mut builder =
            host::facade::Builder::new("proof", host::facade::Abi::new(1).unwrap()).unwrap();
        builder.compatibility_seal([1; 32]).unwrap();
        let definition = builder.finish().unwrap();
        let cycle = host::facade::ProfileCycle::transform(&definition, [1; 32], [2; 32]).unwrap();
        let profile = cycle.finalization(definition.fingerprint()).unwrap();
        let mut fields = Vec::new();
        let mut variants = Vec::new();
        for (index, selector) in selectors.into_iter().enumerate() {
            let mut field = Field::new(
                format!("field{index}").into(),
                Type::builtin(Builtin::I64),
                selector.clone(),
            );
            for ty in [Builtin::I64, Builtin::Bool] {
                // Profile normalization may transform the field type, but not
                // its immutable imported selector.
                field.ty = Type::builtin(ty);
                let expected = writes(&OriginalField {
                    name: &field.name,
                    ty: &field.ty,
                    imported: &field.imported,
                });
                assert_eq!(writes(&field), expected);
                assert_eq!(writes(&field.clone()), expected);
            }
            let variant = Variant::new(
                format!("Variant{index}").into(),
                Box::from([field.clone()]),
                index % 2 == 0,
                selector,
            );
            let expected = writes(&OriginalVariant {
                name: &variant.name,
                fields: &variant.fields,
                constructible: variant.constructible,
                imported: &variant.imported,
            });
            assert_eq!(writes(&variant), expected);
            assert_eq!(writes(&variant.clone()), expected);
            assert_eq!(variant.imported_fingerprint, variant.imported.fingerprint());
            let original: crate::Shared<super::Shape> =
                super::Shape::Enum(Box::from([variant.clone()])).into();
            let mut normalized = original.clone();
            assert_eq!(
                crate::Shared::make_mut(&mut normalized).normalize_profile(&profile),
                0
            );
            assert!(!crate::Shared::ptr_eq(&original, &normalized));
            assert_eq!(writes(original.as_ref()), writes(normalized.as_ref()));
            assert_eq!(original, normalized);
            variants.push(variant);
            fields.push(field);
        }
        for left in &variants {
            for right in &variants {
                assert_eq!(
                    left.cmp(right),
                    (&left.name, &left.fields, left.constructible, &left.imported).cmp(&(
                        &right.name,
                        &right.fields,
                        right.constructible,
                        &right.imported
                    ))
                );
            }
        }
        for left in &fields {
            for right in &fields {
                assert_eq!(
                    left.cmp(right),
                    (&left.name, &left.ty, &left.imported).cmp(&(
                        &right.name,
                        &right.ty,
                        &right.imported
                    ))
                );
            }
        }
    }
}
