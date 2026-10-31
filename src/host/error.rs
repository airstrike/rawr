//! Host registry errors.

use crate::ImmutableString;
use std::fmt;

/// Error returned while registering host metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    /// Compiler host metadata could not reserve its exact retained capacity.
    Allocation,
    /// A type path is empty or contains an invalid identifier segment.
    InvalidTypePath(ImmutableString),
    /// A facade-relative or qualified path is malformed.
    InvalidFacadePath(ImmutableString),
    /// A facade root contains more than one path segment.
    InvalidFacadeRoot(ImmutableString),
    /// Facade ABI versions start at one.
    InvalidFacadeAbi(u32),
    /// Structural tuple metadata requires at least two members.
    InvalidTupleArity {
        /// Number of supplied tuple members.
        actual: usize,
    },
    /// A type parameter is not one identifier.
    InvalidTypeParameter(ImmutableString),
    /// Invocation-scoped facade types must be structurally nongeneric.
    InvocationTypeParameters {
        /// Qualified invocation-scoped type path.
        ty: ImmutableString,
    },
    /// A type parameter name is declared more than once.
    DuplicateTypeParameter {
        /// Item or type declaring the parameter.
        item: ImmutableString,
        /// Reused parameter name.
        parameter: ImmutableString,
    },
    /// A capability declaration omits one of its semantic prerequisites.
    InvalidCapabilitySet {
        /// Type or signature parameter declaring the set.
        declaration: ImmutableString,
        /// Capability whose prerequisite is missing.
        capability: &'static str,
        /// Required companion capability.
        required: &'static str,
    },
    /// A signature references a type parameter it did not declare.
    UnknownTypeParameter {
        /// Item containing the reference.
        item: ImmutableString,
        /// Missing parameter name.
        parameter: ImmutableString,
    },
    /// A signature references no type declared by its facade.
    UnknownFacadeType {
        /// Item containing the reference.
        item: ImmutableString,
        /// Missing type path.
        ty: ImmutableString,
    },
    /// A symbolic imported identity has no definition in this exact facade.
    UnknownImportedType {
        /// Item containing the unresolved semantic reference.
        item: ImmutableString,
        /// Stable imported definition identity.
        identity: [u8; 32],
    },
    /// An applied facade type has the wrong number of structural arguments.
    InvalidFacadeTypeArguments {
        /// Item containing the reference.
        item: ImmutableString,
        /// Applied type path.
        ty: ImmutableString,
        /// Declared generic arity.
        expected: usize,
        /// Supplied generic arity.
        actual: usize,
    },
    /// A registered intrinsic list macro does not declare one repeated input.
    InvalidIntrinsicMacroSignature {
        /// Qualified macro item path.
        item: ImmutableString,
        /// Number of declared positional input types.
        actual: usize,
    },
    /// An elidable producer lacks the exact inert signature contract.
    InvalidElidableSignature {
        /// Qualified item path.
        item: ImmutableString,
    },
    /// A nested declaration has no declared parent module.
    MissingFacadeModule {
        /// Declaration path.
        path: ImmutableString,
        /// Required module path.
        module: ImmutableString,
    },
    /// A method or associated function does not belong to a declared type.
    InvalidAssociatedOwner {
        /// Associated item path.
        item: ImmutableString,
        /// Required owner type path.
        owner: ImmutableString,
    },
    /// Two facade declarations occupy the same semantic path.
    DuplicateFacadeEntry {
        /// Conflicting path.
        path: ImmutableString,
        /// Existing entry kind.
        existing: &'static str,
        /// New entry kind.
        incoming: &'static str,
    },
    /// Declarations at one overloaded path disagree on item kind or receiver.
    IncompatibleFacadeOverload {
        /// Qualified overloaded item path.
        item: ImmutableString,
        /// Kind of the first declaration.
        existing_kind: super::item::Kind,
        /// Receiver of the first declaration.
        existing_receiver: Option<crate::receiver::Mode>,
        /// Kind of the incoming declaration.
        incoming_kind: super::item::Kind,
        /// Receiver of the incoming declaration.
        incoming_receiver: Option<crate::receiver::Mode>,
    },
    /// Two declarations at one path differ only outside their exact inputs.
    DuplicateFacadeOverloadInputs {
        /// Qualified overloaded item path.
        item: ImmutableString,
    },
    /// One Rust type identity was assigned incompatible compiler contracts.
    ConflictingHostIdentity {
        /// First registered path.
        existing: ImmutableString,
        /// Conflicting registered path.
        incoming: ImmutableString,
    },
    /// A projection names no type declared by its source facade.
    UnknownProjectionSource {
        /// Missing source type path.
        source: ImmutableString,
    },
    /// A projection maps a parameter absent from its source type.
    InvalidProjectionParameter {
        /// Source type owning the projection.
        source: ImmutableString,
        /// Target type receiving the mapped argument.
        target: ImmutableString,
        /// Unknown source parameter.
        parameter: ImmutableString,
    },
    /// A source type declares the same direct target more than once.
    DuplicateProjection {
        /// Source type owning the duplicate edge.
        source: ImmutableString,
        /// Repeated direct target type.
        target: ImmutableString,
    },
    /// Local direct projections contain a dependency cycle.
    ProjectionCycle {
        /// Type at which the cycle was detected.
        ty: ImmutableString,
    },
    /// A referenced external facade dependency is not installed.
    MissingFacadeDependency {
        /// Facade declaration retaining the external proof.
        source: ImmutableString,
        /// Required target facade root.
        facade: ImmutableString,
    },
    /// An installed external dependency differs from its exported proof.
    FacadeDependencyMismatch {
        /// Facade declaration retaining the external proof.
        source: ImmutableString,
        /// Required target type.
        target: ImmutableString,
    },
    /// A finished facade conflicts with compiler metadata already installed.
    RegistryConflict {
        /// Conflicting qualified path.
        path: ImmutableString,
        /// Existing registry entry kind.
        existing: &'static str,
        /// Incoming facade entry kind.
        incoming: &'static str,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Allocation => formatter.write_str("cannot allocate compiler host metadata"),
            Self::InvalidTypePath(path) => {
                write!(formatter, "invalid host type path `{path}`")
            }
            Self::InvalidFacadePath(path) => write!(formatter, "invalid path `{path}`"),
            Self::InvalidFacadeRoot(path) => {
                write!(formatter, "an export root must be one identifier, got `{path}`")
            }
            Self::InvalidFacadeAbi(version) => {
                write!(formatter, "an export ABI version must be non-zero, got {version}")
            }
            Self::InvalidTupleArity { actual } => write!(
                formatter,
                "structural tuples require at least 2 members, found {actual}"
            ),
            Self::InvalidTypeParameter(parameter) => {
                write!(formatter, "invalid host type parameter `{parameter}`")
            }
            Self::InvocationTypeParameters { ty } => write!(
                formatter,
                "invocation-scoped host type `{ty}` cannot declare type parameters"
            ),
            Self::DuplicateTypeParameter { item, parameter } => write!(
                formatter,
                "duplicate type parameter `{parameter}` in host declaration `{item}`"
            ),
            Self::InvalidCapabilitySet {
                declaration,
                capability,
                required,
            } => write!(
                formatter,
                "invalid capability set for `{declaration}`: `{capability}` requires `{required}`"
            ),
            Self::UnknownTypeParameter { item, parameter } => write!(
                formatter,
                "unknown type parameter `{parameter}` in host item `{item}`"
            ),
            Self::UnknownFacadeType { item, ty } => {
                write!(formatter, "unknown type `{ty}` in host item `{item}`")
            }
            Self::UnknownImportedType { item, identity } => {
                write!(formatter, "unknown imported type `")?;
                for byte in identity {
                    write!(formatter, "{byte:02x}")?;
                }
                write!(formatter, "` in host item `{item}`")
            }
            Self::InvalidFacadeTypeArguments {
                item,
                ty,
                expected,
                actual,
            } => write!(
                formatter,
                "invalid type arguments for `{ty}` in host item `{item}`: expected {expected}, found {actual}"
            ),
            Self::InvalidIntrinsicMacroSignature { item, actual } => write!(
                formatter,
                "intrinsic list macro `{item}` must declare exactly 1 repeated input type, found {actual}"
            ),
            Self::InvalidElidableSignature { item } => write!(
                formatter,
                "elidable host producer `{item}` must be discardable, receiver-free, and parameterless"
            ),
            Self::MissingFacadeModule { path, module } => write!(
                formatter,
                "host declaration `{path}` requires missing module `{module}`"
            ),
            Self::InvalidAssociatedOwner { item, owner } => write!(
                formatter,
                "host item `{item}` requires declared owner type `{owner}`"
            ),
            Self::DuplicateFacadeEntry {
                path,
                existing,
                incoming,
            } => write!(
                formatter,
                "`{path}` is declared twice: {incoming} conflicts with {existing}"
            ),
            Self::IncompatibleFacadeOverload {
                item,
                existing_kind,
                existing_receiver,
                incoming_kind,
                incoming_receiver,
            } => write!(
                formatter,
                "incompatible overloads of `{item}`: {incoming_kind:?}/{incoming_receiver:?} conflicts with {existing_kind:?}/{existing_receiver:?}"
            ),
            Self::DuplicateFacadeOverloadInputs { item } => write!(
                formatter,
                "an overload of `{item}` repeats an exact input signature; return-only overloads are not allowed"
            ),
            Self::ConflictingHostIdentity { existing, incoming } => write!(
                formatter,
                "host type `{incoming}` conflicts with the compiler contract for `{existing}`"
            ),
            Self::UnknownProjectionSource { source } => {
                write!(formatter, "unknown projection source type `{source}`")
            }
            Self::InvalidProjectionParameter {
                source,
                target,
                parameter,
            } => write!(
                formatter,
                "invalid projection from `{source}` to `{target}`: unknown source parameter `{parameter}`"
            ),
            Self::DuplicateProjection { source, target } => write!(
                formatter,
                "duplicate direct projection from `{source}` to `{target}`"
            ),
            Self::ProjectionCycle { ty } => {
                write!(formatter, "projection cycle includes `{ty}`")
            }
            Self::MissingFacadeDependency { source, facade: root } => write!(
                formatter,
                "`{source}` requires `{root}`, which is not installed"
            ),
            Self::FacadeDependencyMismatch { source, target } => write!(
                formatter,
                "`{source}` requires the exact exported contract for `{target}`"
            ),
            Self::RegistryConflict {
                path,
                existing,
                incoming,
            } => write!(
                formatter,
                "cannot install {incoming} `{path}` over registered {existing}"
            ),
        }
    }
}

impl std::error::Error for Error {}
