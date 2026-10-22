//! Canonical normal-build Rust API shapes retained before representability.

use serde::{Deserialize, Serialize};

use super::{identity, Retained, RetainedCategory};

/// A bounded byte- and line-positioned source range.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct Span {
    file: Box<str>,
    byte_start: u32,
    byte_end: u32,
    line_start: u32,
    column_start: u32,
    line_end: u32,
    column_end: u32,
}

/// Compiler origin evidence for a normal-build item.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum Origin {
    /// The item was written at its retained source span.
    Source,
    /// The item came from macro expansion with both call and definition sites.
    Expansion {
        /// The position that invoked the expansion.
        call_site: Span,
        /// The macro definition position.
        definition_site: Span,
    },
    /// The compiler synthesized the item from a named language mechanism.
    Synthesized(Box<str>),
}

/// Effective visibility after compiler name resolution.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum Visibility {
    /// Reachable from outside the defining crate.
    Public,
    /// Visible only within one canonical definition scope.
    Restricted(identity::Definition),
}

/// Whether a definition is free or owned by another definition/implementation.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum Owner {
    /// A crate/module-level definition.
    Free,
    /// A definition owned by a canonical parent item.
    Definition(identity::Definition),
    /// A definition owned by one canonical implementation edge.
    Implementation(identity::Definition),
}

/// A complete canonical definition ledger entry.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct Definition {
    id: identity::Definition,
    display_path: Box<[Box<str>]>,
    owner: Owner,
    visibility: Visibility,
    kind: Kind,
    span: Span,
    origin: Origin,
}

/// A complete normal Rust definition shape.
#[allow(
    clippy::large_enum_variant,
    reason = "canonical definition shapes remain inline in the immutable proof model"
)]
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum Kind {
    /// A declaration retained after its compiler shape failed closed.
    Rejected {
        /// The exact operation class recoverable from the compiler declaration.
        class: RejectedClass,
        /// Stable extraction evidence for why no usable shape exists.
        reason: super::ledger::Reason,
    },
    /// A module definition.
    Module,
    /// A struct definition with exact fields and generic evidence.
    Struct {
        /// Exact constructor form, public fields, and completeness evidence.
        fields: FieldSet,
        /// Type, lifetime, const, bound, and where-clause evidence.
        generics: Generics,
    },
    /// A union definition with exact fields and generic evidence.
    Union {
        /// Exact record form, public fields, and completeness evidence.
        fields: FieldSet,
        /// Type, lifetime, const, bound, and where-clause evidence.
        generics: Generics,
    },
    /// An enum definition with variants and discriminants.
    Enum {
        /// Ordered canonical variants.
        variants: Box<[Variant]>,
        /// Type, lifetime, const, bound, and where-clause evidence.
        generics: Generics,
    },
    /// A constant definition.
    Const {
        /// The exact compiler type.
        ty: Type,
        /// Explicit presence or absence of a constant value.
        value: Default<Const>,
    },
    /// A compiler-authenticated type-level constant retained only as semantic support.
    TypeConst {
        /// The declared type of the type-level constant.
        ty: Type,
        /// Exact presence or absence of the type-system right-hand side.
        rhs: Default<Const>,
    },
    /// A static definition.
    Static {
        /// The exact compiler type.
        ty: Type,
        /// Whether mutation requires unsafe Rust access.
        mutable: bool,
        /// Whether the static is thread-local.
        thread_local: bool,
    },
    /// A type alias.
    TypeAlias {
        /// The aliased type.
        ty: Type,
        /// Type, lifetime, const, bound, and where-clause evidence.
        generics: Generics,
    },
    /// A free or associated function.
    Function {
        /// The exact callable signature.
        signature: Signature,
        /// Type, lifetime, const, bound, and where-clause evidence.
        generics: Generics,
    },
    /// A trait with supertraits and associated definitions.
    Trait {
        /// Whether implementing the trait is unsafe.
        unsafe_trait: bool,
        /// Whether the trait is automatically implemented.
        auto_trait: bool,
        /// Exact semantic supertrait predicates.
        supertraits: Box<[Predicate]>,
        /// Canonical associated definition identities.
        associated: Box<[identity::Definition]>,
        /// Type, lifetime, const, bound, and where-clause evidence.
        generics: Generics,
    },
    /// An associated type declaration or definition.
    AssociatedType {
        /// Exact semantic predicates declared on the associated type.
        bounds: Box<[Predicate]>,
        /// Explicit presence or absence of a default type.
        default: Default<Type>,
        /// Type, lifetime, const, bound, and where-clause evidence.
        generics: Generics,
    },
    /// A compiler-shipped nominal type retained as an API-relevant leaf.
    Sysroot {
        /// rustc's exact normal type classification.
        compiler_kind: Box<str>,
    },
}

/// Closed operation class retained for a rejected declaration.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum RejectedClass {
    /// A nominal declaration with no directly callable operation.
    Declaration,
    /// A free function.
    Function,
    /// A free constant.
    Constant,
    /// A method with an exact receiver ownership class.
    Method(MethodReceiver),
    /// An implementation- or definition-associated function.
    AssociatedFunction,
    /// An implementation- or definition-associated constant.
    AssociatedConstant,
    /// The compiler evidence did not expose a demandable operation class.
    Unavailable,
}

/// Receiver ownership retained even when the rest of a method shape is rejected.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum MethodReceiver {
    /// `self` is consumed by value.
    Owned,
    /// `self` is borrowed immutably.
    Shared,
    /// `self` is borrowed mutably.
    Mutable,
}

/// Explicit default-value evidence without optional proof fields.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum Default<T> {
    /// No default exists in the compiler definition.
    Absent,
    /// The compiler proved this exact default.
    Present(T),
}

/// One API-nameable struct, union, enum-variant, or tuple field.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct Field {
    name: Box<str>,
    index: u32,
    visibility: Visibility,
    ty: Type,
    span: Span,
}

/// Closed source grammar for a struct or enum-variant constructor.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum FieldForm {
    /// Named fields enclosed in braces.
    Record,
    /// Positional fields enclosed in parentheses.
    Tuple,
    /// No payload fields and no delimiters.
    Unit,
}

/// Exact public field surface plus non-leaking constructor completeness proof.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct FieldSet {
    form: FieldForm,
    public: Box<[Field]>,
    public_complete: bool,
    non_exhaustive: bool,
}

/// One enum variant and optional explicit discriminant.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct Variant {
    name: Box<str>,
    fields: FieldSet,
    discriminant: Default<Const>,
    span: Span,
}

/// A complete function signature.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct Signature {
    safety: Safety,
    const_function: bool,
    async_function: bool,
    abi: Box<str>,
    variadic: bool,
    binders: Box<[Binder]>,
    receiver: Receiver,
    inputs: Box<[Type]>,
    output: Type,
}

/// Function or implementation safety.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum Safety {
    /// Safe Rust may use the item.
    Safe,
    /// The item requires an unsafe obligation.
    Unsafe,
}

/// Exact receiver evidence for a function.
#[allow(
    clippy::large_enum_variant,
    reason = "receiver type evidence is an inseparable part of the canonical signature"
)]
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum Receiver {
    /// No receiver is present.
    None,
    /// `self` is consumed by value.
    Value {
        /// The exact self type after substitution.
        ty: Type,
    },
    /// `self` is borrowed.
    Reference {
        /// The exact binder-aware receiver lifetime.
        lifetime: Lifetime,
        /// Shared or mutable borrow.
        mutability: Mutability,
        /// The exact self type after substitution.
        ty: Type,
    },
    /// An explicit arbitrary self type such as `Pin<&mut Self>`.
    Explicit {
        /// The compiler-resolved receiver type.
        ty: Type,
    },
}

/// Shared versus mutable access.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum Mutability {
    /// Shared or immutable access.
    Shared,
    /// Mutable access.
    Mutable,
}

/// Type, lifetime, const, bound, and where-predicate evidence.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct Generics {
    parameters: Box<[Parameter]>,
    predicates: Box<[Predicate]>,
}

/// One canonical generic parameter.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct Parameter {
    owner: identity::Definition,
    index: u32,
    name: Box<str>,
    kind: ParameterKind,
}

/// The closed kinds of Rust generic parameters.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum ParameterKind {
    /// A lifetime parameter with its declared outlives bounds.
    Lifetime {
        /// Declared lifetime bounds.
        bounds: Box<[Bound]>,
    },
    /// A type parameter with bounds and explicit default evidence.
    Type {
        /// Declared bounds.
        bounds: Box<[Bound]>,
        /// Explicit default evidence.
        default: Default<Type>,
    },
    /// A const parameter with its type and explicit default evidence.
    Const {
        /// The const parameter's type.
        ty: Type,
        /// Explicit default evidence.
        default: Default<Const>,
    },
}

/// One compiler-normalized where predicate.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum Predicate {
    /// A type satisfies a trait or lifetime bound.
    Bound { ty: Type, bound: Bound },
    /// A projection normalizes to a compiler type.
    ProjectionEquality { projection: Type, ty: Type },
    /// One lifetime outlives another.
    LifetimeOutlives { longer: Lifetime, shorter: Lifetime },
    /// A type outlives a lifetime.
    TypeOutlives { ty: Type, lifetime: Lifetime },
    /// A const expression is evaluatable.
    ConstEvaluatable(Const),
}

/// One trait, lifetime, or marker bound.
#[allow(
    clippy::large_enum_variant,
    reason = "trait identity and substitutions form one inline canonical bound"
)]
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum Bound {
    /// A trait bound with exact substitutions and polarity.
    Trait {
        /// The referenced trait definition.
        trait_id: identity::Definition,
        /// Exact generic substitutions.
        arguments: Box<[Argument]>,
        /// Whether the bound is positive or negative.
        polarity: Polarity,
        /// Higher-ranked binders introduced by the bound.
        binders: Box<[Binder]>,
    },
    /// The `?Sized` relaxation.
    MaybeSized,
}

/// One closed semantic predicate attached to a trait object.
#[allow(
    clippy::large_enum_variant,
    reason = "trait-object projection evidence remains inline in the canonical model"
)]
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum ObjectBound {
    /// A principal or auto-trait bound.
    Trait(Bound),
    /// An associated type binding such as `Iterator<Item = u8>`.
    ProjectionEquality {
        associated: identity::Definition,
        arguments: Box<[Argument]>,
        ty: Type,
    },
}

/// Trait and implementation polarity.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum Polarity {
    /// Positive trait evidence.
    Positive,
    /// Negative trait evidence.
    Negative,
}

/// A stable higher-ranked binder identity.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct Binder {
    owner: identity::Definition,
    depth: u32,
    index: u32,
    name: Box<str>,
}

/// A binder-aware lifetime identity.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum Lifetime {
    /// The static lifetime.
    Static,
    /// An early-bound generic lifetime.
    Early {
        owner: identity::Definition,
        index: u32,
    },
    /// A late-bound higher-ranked lifetime.
    Late { binder: Binder, index: u32 },
    /// An inference-erased lifetime retained explicitly by rustc.
    Erased,
}

/// A complete compiler type shape.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum Type {
    /// A compiler primitive such as `u32` or `str`.
    Primitive(Box<str>),
    /// A canonical nominal definition with exact arguments.
    Definition {
        /// The referenced nominal definition.
        id: identity::Definition,
        /// Exact lifetime/type/const substitutions.
        arguments: Box<[Argument]>,
    },
    /// A type generic parameter.
    Parameter {
        owner: identity::Definition,
        index: u32,
    },
    /// The `Self` type belonging to a canonical owner.
    SelfType(identity::Definition),
    /// A tuple, including unit as the empty tuple.
    Tuple(Box<[Type]>),
    /// A slice type.
    Slice(Box<Type>),
    /// An array type and exact const length.
    Array { element: Box<Type>, length: Const },
    /// A reference type.
    Reference {
        /// The binder-aware lifetime.
        lifetime: Lifetime,
        /// Shared or mutable borrow.
        mutability: Mutability,
        /// The referenced type.
        ty: Box<Type>,
    },
    /// A raw pointer type.
    RawPointer {
        /// Const or mutable pointer.
        mutability: Mutability,
        /// The pointed-to type.
        ty: Box<Type>,
    },
    /// A bare function pointer.
    FunctionPointer(Box<Signature>),
    /// A trait object.
    TraitObject {
        /// Exact principal, auto-trait, and associated-binding evidence.
        bounds: Box<[ObjectBound]>,
        /// Object lifetime.
        lifetime: Lifetime,
    },
    /// An opaque `impl Trait` identity and its bounds.
    ImplTrait {
        /// The compiler definition owning the opaque type.
        id: identity::Definition,
        /// Exact semantic public predicates.
        bounds: Box<[Predicate]>,
        /// Exact compiler arguments captured by this opaque type.
        captures: Box<[Argument]>,
    },
    /// An associated-type projection.
    Projection {
        /// The trait whose item is projected.
        trait_id: identity::Definition,
        /// The associated type definition.
        associated: identity::Definition,
        /// Exact substitutions.
        arguments: Box<[Argument]>,
    },
    /// The never type.
    Never,
}

/// One exact generic substitution.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum Argument {
    /// A lifetime argument.
    Lifetime(Lifetime),
    /// A type argument.
    Type(Type),
    /// A const argument.
    Const(Const),
}

/// Canonical constant evidence retained from rustc.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum Const {
    /// A compiler-canonical literal spelling and type.
    Literal { ty: Box<str>, value: Box<str> },
    /// A const generic parameter.
    Parameter {
        owner: identity::Definition,
        index: u32,
    },
    /// An unevaluated const item with exact substitutions.
    Unevaluated {
        /// The defining const item.
        definition: identity::Definition,
        /// Exact substitutions.
        arguments: Box<[Argument]>,
    },
}

/// One canonical inherent or trait implementation edge.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct Implementation {
    key: Box<str>,
    id: identity::Definition,
    relevance: Box<[identity::Definition]>,
    trait_ref: Trait,
    self_type: Type,
    safety: Safety,
    polarity: Polarity,
    const_impl: bool,
    default_impl: bool,
    generics: Generics,
    associated: Box<[identity::Definition]>,
    span: Span,
    origin: Origin,
}

/// One completed compiler query for a selected symbolic predicate instantiated
/// at a member of the closed representable scalar universe.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct ScalarPredicateProof {
    owner: identity::Definition,
    parameter: u32,
    trait_id: identity::Definition,
    scalar: Scalar,
    outcome: ScalarPredicateOutcome,
}

/// The versioned closed scalar universe eligible for predicate proof queries.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum Scalar {
    Unit,
    Bool,
    Char,
    F64,
    I64,
}

/// Exact terminal result of one completed scalar predicate query.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum ScalarPredicateOutcome {
    Absent,
    Positive {
        implementation: identity::Definition,
    },
    Unresolved,
}

/// Exact inherent-versus-trait implementation evidence.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum Trait {
    /// An inherent implementation.
    Inherent,
    /// A trait implementation with substitutions.
    Trait {
        /// The implemented trait.
        id: identity::Definition,
        /// Exact trait substitutions.
        arguments: Box<[Argument]>,
    },
}

impl Span {
    pub(super) fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        retained.string(&self.file)
    }
}

impl Origin {
    fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        match self {
            Self::Source => Some(()),
            Self::Expansion {
                call_site,
                definition_site,
            } => {
                call_site.retain(retained)?;
                definition_site.retain(retained)
            }
            Self::Synthesized(mechanism) => retained.string(mechanism),
        }
    }
}

impl Visibility {
    fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        match self {
            Self::Public => Some(()),
            Self::Restricted(definition) => definition.retain(retained),
        }
    }
}

impl Owner {
    fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        match self {
            Self::Free => Some(()),
            Self::Definition(definition) | Self::Implementation(definition) => {
                definition.retain(retained)
            }
        }
    }
}

impl Definition {
    pub(super) fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        self.id.retain(retained)?;
        retained.slice(&self.display_path)?;
        for segment in &self.display_path {
            retained.string(segment)?;
        }
        self.owner.retain(retained)?;
        self.visibility.retain(retained)?;
        self.kind.retain(retained)?;
        self.span.retain(retained)?;
        self.origin.retain(retained)
    }
}

impl Kind {
    fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.category(RetainedCategory::Kind, |retained| {
            retained.node()?;
            match self {
                Self::Rejected { reason, .. } => reason.retain(retained),
                Self::Module => Some(()),
                Self::Struct { fields, generics } | Self::Union { fields, generics } => {
                    fields.retain(retained)?;
                    generics.retain(retained)
                }
                Self::Enum { variants, generics } => {
                    retained.slice(variants)?;
                    for variant in variants {
                        variant.retain(retained)?;
                    }
                    generics.retain(retained)
                }
                Self::Const { ty, value } => {
                    ty.retain(retained)?;
                    retain_default_const(value, retained)
                }
                Self::TypeConst { ty, rhs } => {
                    ty.retain(retained)?;
                    retain_default_const(rhs, retained)
                }
                Self::Static { ty, .. } => ty.retain(retained),
                Self::TypeAlias { ty, generics } => {
                    ty.retain(retained)?;
                    generics.retain(retained)
                }
                Self::Function {
                    signature,
                    generics,
                } => {
                    signature.retain(retained)?;
                    generics.retain(retained)
                }
                Self::Trait {
                    supertraits,
                    associated,
                    generics,
                    ..
                } => {
                    retained.slice(supertraits)?;
                    for predicate in supertraits {
                        predicate.retain(retained)?;
                    }
                    retained.slice(associated)?;
                    for definition in associated {
                        definition.retain(retained)?;
                    }
                    generics.retain(retained)
                }
                Self::AssociatedType {
                    bounds,
                    default,
                    generics,
                } => {
                    retained.slice(bounds)?;
                    for predicate in bounds {
                        predicate.retain(retained)?;
                    }
                    retain_default_type(default, retained)?;
                    generics.retain(retained)
                }
                Self::Sysroot { compiler_kind } => retained.string(compiler_kind),
            }
        })
    }
}

impl Field {
    fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        retained.string(&self.name)?;
        self.visibility.retain(retained)?;
        self.ty.retain(retained)?;
        self.span.retain(retained)
    }
}

impl FieldSet {
    fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        retained.slice(&self.public)?;
        for field in &self.public {
            field.retain(retained)?;
        }
        Some(())
    }
}

impl Variant {
    fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        retained.string(&self.name)?;
        self.fields.retain(retained)?;
        retain_default_const(&self.discriminant, retained)?;
        self.span.retain(retained)
    }
}

impl Signature {
    fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        retained.string(&self.abi)?;
        retained.slice(&self.binders)?;
        for binder in &self.binders {
            binder.retain(retained)?;
        }
        self.receiver.retain(retained)?;
        retained.slice(&self.inputs)?;
        for input in &self.inputs {
            input.retain(retained)?;
        }
        self.output.retain(retained)
    }
}

impl Receiver {
    fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        match self {
            Self::None => Some(()),
            Self::Value { ty } | Self::Explicit { ty } => ty.retain(retained),
            Self::Reference { lifetime, ty, .. } => {
                lifetime.retain(retained)?;
                ty.retain(retained)
            }
        }
    }
}

impl Generics {
    fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.category(RetainedCategory::Generic, |retained| {
            retained.node()?;
            retained.slice(&self.parameters)?;
            for parameter in &self.parameters {
                parameter.retain(retained)?;
            }
            retained.slice(&self.predicates)?;
            for predicate in &self.predicates {
                predicate.retain(retained)?;
            }
            Some(())
        })
    }
}

impl Parameter {
    fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        self.owner.retain(retained)?;
        retained.string(&self.name)?;
        self.kind.retain(retained)
    }
}

impl ParameterKind {
    fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        match self {
            Self::Lifetime { bounds } => retain_bounds(bounds, retained),
            Self::Type { bounds, default } => {
                retain_bounds(bounds, retained)?;
                retain_default_type(default, retained)
            }
            Self::Const { ty, default } => {
                ty.retain(retained)?;
                retain_default_const(default, retained)
            }
        }
    }
}

impl Predicate {
    fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.category(RetainedCategory::Predicate, |retained| {
            retained.node()?;
            match self {
                Self::Bound { ty, bound } => {
                    ty.retain(retained)?;
                    bound.retain(retained)
                }
                Self::ProjectionEquality { projection, ty } => {
                    projection.retain(retained)?;
                    ty.retain(retained)
                }
                Self::LifetimeOutlives { longer, shorter } => {
                    longer.retain(retained)?;
                    shorter.retain(retained)
                }
                Self::TypeOutlives { ty, lifetime } => {
                    ty.retain(retained)?;
                    lifetime.retain(retained)
                }
                Self::ConstEvaluatable(value) => value.retain(retained),
            }
        })
    }
}

impl Bound {
    fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.category(RetainedCategory::Predicate, |retained| {
            retained.node()?;
            match self {
                Self::Trait {
                    trait_id,
                    arguments,
                    binders,
                    ..
                } => {
                    trait_id.retain(retained)?;
                    retain_arguments(arguments, retained)?;
                    retained.slice(binders)?;
                    for binder in binders {
                        binder.retain(retained)?;
                    }
                    Some(())
                }
                Self::MaybeSized => Some(()),
            }
        })
    }
}

impl ObjectBound {
    fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.category(RetainedCategory::Predicate, |retained| {
            retained.node()?;
            match self {
                Self::Trait(bound) => bound.retain(retained),
                Self::ProjectionEquality {
                    associated,
                    arguments,
                    ty,
                } => {
                    associated.retain(retained)?;
                    retain_arguments(arguments, retained)?;
                    ty.retain(retained)
                }
            }
        })
    }
}

impl Binder {
    fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        self.owner.retain(retained)?;
        retained.string(&self.name)
    }
}

impl Lifetime {
    fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        match self {
            Self::Static | Self::Erased => Some(()),
            Self::Early { owner, .. } => owner.retain(retained),
            Self::Late { binder, .. } => binder.retain(retained),
        }
    }
}

impl Type {
    fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.category(RetainedCategory::Type, |retained| {
            retained.node()?;
            match self {
                Self::Primitive(name) => retained.string(name),
                Self::Definition { id, arguments } => {
                    id.retain(retained)?;
                    retain_arguments(arguments, retained)
                }
                Self::Parameter { owner, .. } | Self::SelfType(owner) => owner.retain(retained),
                Self::Tuple(elements) => {
                    retained.slice(elements)?;
                    for element in elements {
                        element.retain(retained)?;
                    }
                    Some(())
                }
                Self::Slice(ty) | Self::RawPointer { ty, .. } => {
                    retained.boxed::<Type>()?;
                    ty.retain(retained)
                }
                Self::Array { element, length } => {
                    retained.boxed::<Type>()?;
                    element.retain(retained)?;
                    length.retain(retained)
                }
                Self::Reference { lifetime, ty, .. } => {
                    lifetime.retain(retained)?;
                    retained.boxed::<Type>()?;
                    ty.retain(retained)
                }
                Self::FunctionPointer(signature) => {
                    retained.boxed::<Signature>()?;
                    signature.retain(retained)
                }
                Self::TraitObject { bounds, lifetime } => {
                    retained.slice(bounds)?;
                    for bound in bounds {
                        bound.retain(retained)?;
                    }
                    lifetime.retain(retained)
                }
                Self::ImplTrait {
                    id,
                    bounds,
                    captures,
                } => {
                    id.retain(retained)?;
                    retained.slice(bounds)?;
                    for predicate in bounds {
                        predicate.retain(retained)?;
                    }
                    retain_arguments(captures, retained)
                }
                Self::Projection {
                    trait_id,
                    associated,
                    arguments,
                } => {
                    trait_id.retain(retained)?;
                    associated.retain(retained)?;
                    retain_arguments(arguments, retained)
                }
                Self::Never => Some(()),
            }
        })
    }
}

impl Argument {
    fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.category(RetainedCategory::Instantiation, |retained| {
            retained.node()?;
            match self {
                Self::Lifetime(lifetime) => lifetime.retain(retained),
                Self::Type(ty) => ty.retain(retained),
                Self::Const(value) => value.retain(retained),
            }
        })
    }
}

impl Const {
    fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.category(RetainedCategory::Instantiation, |retained| {
            retained.node()?;
            match self {
                Self::Literal { ty, value } => {
                    retained.string(ty)?;
                    retained.string(value)
                }
                Self::Parameter { owner, .. } => owner.retain(retained),
                Self::Unevaluated {
                    definition,
                    arguments,
                } => {
                    definition.retain(retained)?;
                    retain_arguments(arguments, retained)
                }
            }
        })
    }
}

impl Implementation {
    pub(super) fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        retained.string(&self.key)?;
        self.id.retain(retained)?;
        retained.slice(&self.relevance)?;
        for definition in &self.relevance {
            definition.retain(retained)?;
        }
        self.trait_ref.retain(retained)?;
        self.self_type.retain(retained)?;
        self.generics.retain(retained)?;
        retained.slice(&self.associated)?;
        for definition in &self.associated {
            definition.retain(retained)?;
        }
        self.span.retain(retained)?;
        self.origin.retain(retained)
    }
}

impl ScalarPredicateProof {
    pub(super) fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        self.owner.retain(retained)?;
        self.trait_id.retain(retained)?;
        match &self.outcome {
            ScalarPredicateOutcome::Positive { implementation } => implementation.retain(retained),
            ScalarPredicateOutcome::Absent | ScalarPredicateOutcome::Unresolved => Some(()),
        }
    }
}

impl Trait {
    fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        match self {
            Self::Inherent => Some(()),
            Self::Trait { id, arguments } => {
                id.retain(retained)?;
                retain_arguments(arguments, retained)
            }
        }
    }
}

fn retain_arguments(arguments: &[Argument], retained: &mut Retained) -> Option<()> {
    retained.category(RetainedCategory::Instantiation, |retained| {
        retained.slice(arguments)?;
        for argument in arguments {
            argument.retain(retained)?;
        }
        Some(())
    })
}

fn retain_bounds(bounds: &[Bound], retained: &mut Retained) -> Option<()> {
    retained.slice(bounds)?;
    for bound in bounds {
        bound.retain(retained)?;
    }
    Some(())
}

fn retain_default_type(value: &Default<Type>, retained: &mut Retained) -> Option<()> {
    retained.node()?;
    match value {
        Default::Absent => Some(()),
        Default::Present(ty) => ty.retain(retained),
    }
}

fn retain_default_const(value: &Default<Const>, retained: &mut Retained) -> Option<()> {
    retained.node()?;
    match value {
        Default::Absent => Some(()),
        Default::Present(value) => value.retain(retained),
    }
}

impl Span {
    pub(crate) fn retained_bytes(&self) -> usize {
        self.file.len()
    }

    pub(crate) fn new(
        file: Box<str>,
        byte_start: u32,
        byte_end: u32,
        line_start: u32,
        column_start: u32,
        line_end: u32,
        column_end: u32,
    ) -> Self {
        Self {
            file,
            byte_start,
            byte_end,
            line_start,
            column_start,
            line_end,
            column_end,
        }
    }

    /// The compiler metadata diagnostic filename.
    pub fn file(&self) -> &str {
        &self.file
    }

    /// The half-open byte range.
    pub const fn byte_range(&self) -> std::ops::Range<u32> {
        self.byte_start..self.byte_end
    }

    /// The one-based line and zero-based column endpoints.
    pub const fn positions(&self) -> ((u32, u32), (u32, u32)) {
        (
            (self.line_start, self.column_start),
            (self.line_end, self.column_end),
        )
    }
}

impl ScalarPredicateProof {
    pub(crate) const fn new(
        owner: identity::Definition,
        parameter: u32,
        trait_id: identity::Definition,
        scalar: Scalar,
        outcome: ScalarPredicateOutcome,
    ) -> Self {
        Self {
            owner,
            parameter,
            trait_id,
            scalar,
            outcome,
        }
    }

    #[must_use]
    pub const fn owner(&self) -> &identity::Definition {
        &self.owner
    }

    #[must_use]
    pub const fn parameter(&self) -> u32 {
        self.parameter
    }

    #[must_use]
    pub const fn trait_id(&self) -> &identity::Definition {
        &self.trait_id
    }

    #[must_use]
    pub const fn scalar(&self) -> Scalar {
        self.scalar
    }

    #[must_use]
    pub const fn outcome(&self) -> &ScalarPredicateOutcome {
        &self.outcome
    }
}

impl Definition {
    /// Whether two extractions of one definition differ only in display path,
    /// which each fragment spells relative to its own selected crate.
    pub(crate) fn same_except_display_path(&self, other: &Self) -> bool {
        self.id == other.id
            && self.owner == other.owner
            && self.visibility == other.visibility
            && self.kind == other.kind
            && self.span == other.span
            && self.origin == other.origin
    }

    pub(crate) fn new(
        id: identity::Definition,
        display_path: Box<[Box<str>]>,
        owner: Owner,
        visibility: Visibility,
        kind: Kind,
        span: Span,
        origin: Origin,
    ) -> Self {
        Self {
            id,
            display_path,
            owner,
            visibility,
            kind,
            span,
            origin,
        }
    }

    /// The canonical rustc definition identity.
    pub const fn id(&self) -> &identity::Definition {
        &self.id
    }

    /// The compiler display path retained as evidence, never identity.
    pub fn display_path(&self) -> impl ExactSizeIterator<Item = &str> {
        self.display_path.iter().map(AsRef::as_ref)
    }

    /// The exact normal-build item shape.
    pub const fn kind(&self) -> &Kind {
        &self.kind
    }

    /// The item's source evidence.
    pub const fn span(&self) -> &Span {
        &self.span
    }

    /// The definition's free, item, or implementation owner.
    pub const fn owner(&self) -> &Owner {
        &self.owner
    }

    /// Effective compiler visibility.
    pub const fn visibility(&self) -> &Visibility {
        &self.visibility
    }

    /// Compiler source or expansion origin.
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    pub(crate) fn for_each_reference(&self, references: &mut dyn FnMut(&identity::Definition)) {
        let mut reference = |candidate: &identity::Definition| {
            if candidate != &self.id {
                references(candidate);
            }
        };
        self.kind.collect_references(&mut reference);
        match &self.owner {
            Owner::Definition(owner) => reference(owner),
            Owner::Free | Owner::Implementation(_) => {}
        }
        if let Visibility::Restricted(scope) = &self.visibility {
            reference(scope);
        }
    }
}

impl Kind {
    fn collect_references(&self, references: &mut dyn FnMut(&identity::Definition)) {
        match self {
            Self::Rejected { .. } | Self::Module => {}
            Self::Struct { fields, generics } | Self::Union { fields, generics } => {
                fields
                    .public
                    .iter()
                    .for_each(|field| field.collect_references(references));
                generics.collect_references(references);
            }
            Self::Enum { variants, generics } => {
                variants
                    .iter()
                    .for_each(|variant| variant.collect_references(references));
                generics.collect_references(references);
            }
            Self::Const { ty, value } | Self::TypeConst { ty, rhs: value } => {
                ty.collect_references(references);
                if let Default::Present(value) = value {
                    value.collect_references(references);
                }
            }
            Self::Static { ty, .. } => ty.collect_references(references),
            Self::TypeAlias { ty, generics } => {
                ty.collect_references(references);
                generics.collect_references(references);
            }
            Self::Function {
                signature,
                generics,
            } => {
                signature.collect_references(references);
                generics.collect_references(references);
            }
            Self::Trait {
                supertraits,
                associated,
                generics,
                ..
            } => {
                supertraits
                    .iter()
                    .for_each(|predicate| predicate.collect_references(references));
                associated.iter().for_each(&mut *references);
                generics.collect_references(references);
            }
            Self::AssociatedType {
                bounds,
                default,
                generics,
            } => {
                bounds
                    .iter()
                    .for_each(|predicate| predicate.collect_references(references));
                if let Default::Present(ty) = default {
                    ty.collect_references(references);
                }
                generics.collect_references(references);
            }
            Self::Sysroot { .. } => {}
        }
    }
}

impl Field {
    pub(crate) fn new(
        name: Box<str>,
        index: u32,
        visibility: Visibility,
        ty: Type,
        span: Span,
    ) -> Self {
        Self {
            name,
            index,
            visibility,
            ty,
            span,
        }
    }

    fn collect_references(&self, references: &mut dyn FnMut(&identity::Definition)) {
        self.ty.collect_references(references);
    }

    /// The source field name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The compiler field index.
    pub const fn index(&self) -> u32 {
        self.index
    }

    /// Effective compiler visibility.
    pub const fn visibility(&self) -> &Visibility {
        &self.visibility
    }

    /// The exact compiler field type.
    pub const fn ty(&self) -> &Type {
        &self.ty
    }

    /// The compiler metadata diagnostic position.
    pub const fn span(&self) -> &Span {
        &self.span
    }
}

impl FieldSet {
    pub(crate) fn new(
        form: FieldForm,
        public: Box<[Field]>,
        public_complete: bool,
        non_exhaustive: bool,
    ) -> Self {
        Self {
            form,
            public,
            public_complete,
            non_exhaustive,
        }
    }

    /// Exact record, tuple, or unit constructor grammar.
    pub const fn form(&self) -> FieldForm {
        self.form
    }

    /// Ordered public fields; private names and types are never retained.
    pub const fn public_fields(&self) -> &[Field] {
        &self.public
    }

    /// Whether the public fields are the complete physical field set.
    pub const fn public_complete(&self) -> bool {
        self.public_complete
    }

    /// Whether Rust forbids external exhaustive construction.
    pub const fn non_exhaustive(&self) -> bool {
        self.non_exhaustive
    }
}

impl Variant {
    pub(crate) fn new(
        name: Box<str>,
        fields: FieldSet,
        discriminant: Default<Const>,
        span: Span,
    ) -> Self {
        Self {
            name,
            fields,
            discriminant,
            span,
        }
    }

    fn collect_references(&self, references: &mut dyn FnMut(&identity::Definition)) {
        self.fields
            .public
            .iter()
            .for_each(|field| field.collect_references(references));
        if let Default::Present(value) = &self.discriminant {
            value.collect_references(references);
        }
    }

    /// The source variant name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Ordered variant fields.
    pub fn fields(&self) -> impl ExactSizeIterator<Item = &Field> {
        self.fields.public.iter()
    }

    /// Exact constructor form and public-completeness evidence.
    pub const fn field_set(&self) -> &FieldSet {
        &self.fields
    }

    /// Explicit compiler discriminant evidence.
    pub const fn discriminant(&self) -> &Default<Const> {
        &self.discriminant
    }

    /// The compiler metadata diagnostic position.
    pub const fn span(&self) -> &Span {
        &self.span
    }
}

impl Signature {
    #[allow(
        clippy::too_many_arguments,
        reason = "the proof-bundle constructor requires every closed signature field at once"
    )]
    pub(crate) fn new(
        safety: Safety,
        const_function: bool,
        async_function: bool,
        abi: Box<str>,
        variadic: bool,
        binders: Box<[Binder]>,
        receiver: Receiver,
        inputs: Box<[Type]>,
        output: Type,
    ) -> Self {
        Self {
            safety,
            const_function,
            async_function,
            abi,
            variadic,
            binders,
            receiver,
            inputs,
            output,
        }
    }

    fn collect_references(&self, references: &mut dyn FnMut(&identity::Definition)) {
        match &self.receiver {
            Receiver::None => {}
            Receiver::Value { ty } | Receiver::Reference { ty, .. } | Receiver::Explicit { ty } => {
                ty.collect_references(references)
            }
        }
        self.inputs
            .iter()
            .for_each(|ty| ty.collect_references(references));
        self.output.collect_references(references);
    }

    /// Safe or unsafe callable evidence.
    pub const fn safety(&self) -> Safety {
        self.safety
    }

    /// Whether this is a `const fn`.
    pub const fn const_function(&self) -> bool {
        self.const_function
    }

    /// Whether this is an `async fn` before return-type normalization.
    pub const fn async_function(&self) -> bool {
        self.async_function
    }

    /// The exact compiler ABI spelling.
    pub fn abi(&self) -> &str {
        &self.abi
    }

    /// Whether the function is variadic.
    pub const fn variadic(&self) -> bool {
        self.variadic
    }

    /// Ordered higher-ranked binders.
    pub fn binders(&self) -> impl ExactSizeIterator<Item = &Binder> {
        self.binders.iter()
    }

    /// Exact receiver evidence.
    pub const fn receiver(&self) -> &Receiver {
        &self.receiver
    }

    /// Ordered input types.
    pub fn inputs(&self) -> impl ExactSizeIterator<Item = &Type> {
        self.inputs.iter()
    }

    /// The exact output type.
    pub const fn output(&self) -> &Type {
        &self.output
    }
}

impl Generics {
    pub(crate) fn new(parameters: Box<[Parameter]>, predicates: Box<[Predicate]>) -> Self {
        Self {
            parameters,
            predicates,
        }
    }

    fn collect_references(&self, references: &mut dyn FnMut(&identity::Definition)) {
        self.parameters
            .iter()
            .for_each(|parameter| parameter.collect_references(references));
        self.predicates
            .iter()
            .for_each(|predicate| predicate.collect_references(references));
    }

    /// Ordered lifetime, type, and const parameters.
    pub fn parameters(&self) -> impl ExactSizeIterator<Item = &Parameter> {
        self.parameters.iter()
    }

    /// Canonically ordered where predicates.
    pub fn predicates(&self) -> impl ExactSizeIterator<Item = &Predicate> {
        self.predicates.iter()
    }

    /// Join exact parent-implementation and item-owned generics in rustc index
    /// order for one associated invocation template.
    pub(crate) fn inherited(parent: &Self, item: &Self) -> Result<Self, ()> {
        let parameter_count = parent
            .parameters
            .len()
            .checked_add(item.parameters.len())
            .ok_or(())?;
        let predicate_count = parent
            .predicates
            .len()
            .checked_add(item.predicates.len())
            .ok_or(())?;
        let mut parameters = Vec::new();
        parameters
            .try_reserve_exact(parameter_count)
            .map_err(|_| ())?;
        parameters.extend(parent.parameters.iter().cloned());
        parameters.extend(item.parameters.iter().cloned());
        if parameters
            .windows(2)
            .any(|pair| pair[0].index() >= pair[1].index())
        {
            return Err(());
        }
        let mut predicates = Vec::new();
        predicates
            .try_reserve_exact(predicate_count)
            .map_err(|_| ())?;
        predicates.extend(parent.predicates.iter().cloned());
        predicates.extend(item.predicates.iter().cloned());
        Ok(Self::new(
            parameters.into_boxed_slice(),
            predicates.into_boxed_slice(),
        ))
    }
}

impl Parameter {
    pub(crate) fn new(
        owner: identity::Definition,
        index: u32,
        name: Box<str>,
        kind: ParameterKind,
    ) -> Self {
        Self {
            owner,
            index,
            name,
            kind,
        }
    }

    fn collect_references(&self, references: &mut dyn FnMut(&identity::Definition)) {
        match &self.kind {
            ParameterKind::Lifetime { bounds } => bounds
                .iter()
                .for_each(|bound| bound.collect_references(references)),
            ParameterKind::Type { bounds, default } => {
                bounds
                    .iter()
                    .for_each(|bound| bound.collect_references(references));
                if let Default::Present(ty) = default {
                    ty.collect_references(references);
                }
            }
            ParameterKind::Const { ty, default } => {
                ty.collect_references(references);
                if let Default::Present(value) = default {
                    value.collect_references(references);
                }
            }
        }
    }

    /// The canonical definition owning this parameter.
    pub const fn owner(&self) -> &identity::Definition {
        &self.owner
    }

    /// The compiler generic index.
    pub const fn index(&self) -> u32 {
        self.index
    }

    /// The source parameter name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Lifetime, type, or const parameter evidence.
    pub const fn kind(&self) -> &ParameterKind {
        &self.kind
    }
}

impl Predicate {
    fn collect_references(&self, references: &mut dyn FnMut(&identity::Definition)) {
        match self {
            Self::Bound { ty, bound } => {
                ty.collect_references(references);
                bound.collect_references(references);
            }
            Self::ProjectionEquality { projection, ty } => {
                projection.collect_references(references);
                ty.collect_references(references);
            }
            Self::LifetimeOutlives { longer, shorter } => {
                longer.collect_references(references);
                shorter.collect_references(references);
            }
            Self::TypeOutlives { ty, lifetime } => {
                ty.collect_references(references);
                lifetime.collect_references(references);
            }
            Self::ConstEvaluatable(value) => value.collect_references(references),
        }
    }
}

impl Bound {
    fn collect_references(&self, references: &mut dyn FnMut(&identity::Definition)) {
        match self {
            Self::Trait {
                trait_id,
                arguments,
                ..
            } => {
                references(trait_id);
                arguments
                    .iter()
                    .for_each(|argument| argument.collect_references(references));
            }
            Self::MaybeSized => {}
        }
    }
}

impl ObjectBound {
    fn collect_references(&self, references: &mut dyn FnMut(&identity::Definition)) {
        match self {
            Self::Trait(bound) => bound.collect_references(references),
            Self::ProjectionEquality {
                associated,
                arguments,
                ty,
            } => {
                references(associated);
                arguments
                    .iter()
                    .for_each(|argument| argument.collect_references(references));
                ty.collect_references(references);
            }
        }
    }
}

impl Lifetime {
    fn collect_references(&self, _references: &mut dyn FnMut(&identity::Definition)) {
        match self {
            Self::Static | Self::Erased => {}
            Self::Early { .. } | Self::Late { .. } => {}
        }
    }
}

impl Binder {
    /// The canonical definition owning this binder.
    pub const fn owner(&self) -> &identity::Definition {
        &self.owner
    }

    /// The higher-ranked binder depth.
    pub const fn depth(&self) -> u32 {
        self.depth
    }

    /// The binder-local compiler index.
    pub const fn index(&self) -> u32 {
        self.index
    }

    /// The source binder name.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl Type {
    fn collect_references(&self, references: &mut dyn FnMut(&identity::Definition)) {
        match self {
            Self::Primitive(_) | Self::Never => {}
            Self::Definition { id, arguments } => {
                references(id);
                arguments
                    .iter()
                    .for_each(|argument| argument.collect_references(references));
            }
            Self::Parameter { .. } | Self::SelfType(_) => {}
            Self::Tuple(types) => types
                .iter()
                .for_each(|ty| ty.collect_references(references)),
            Self::Slice(ty) => ty.collect_references(references),
            Self::Array { element, length } => {
                element.collect_references(references);
                length.collect_references(references);
            }
            Self::Reference { lifetime, ty, .. } => {
                lifetime.collect_references(references);
                ty.collect_references(references);
            }
            Self::RawPointer { ty, .. } => ty.collect_references(references),
            Self::FunctionPointer(signature) => signature.collect_references(references),
            Self::TraitObject { bounds, lifetime } => {
                bounds
                    .iter()
                    .for_each(|bound| bound.collect_references(references));
                lifetime.collect_references(references);
            }
            Self::ImplTrait {
                bounds, captures, ..
            } => {
                bounds
                    .iter()
                    .for_each(|predicate| predicate.collect_references(references));
                captures
                    .iter()
                    .for_each(|argument| argument.collect_references(references));
            }
            Self::Projection {
                trait_id,
                associated,
                arguments,
            } => {
                references(trait_id);
                references(associated);
                arguments
                    .iter()
                    .for_each(|argument| argument.collect_references(references));
            }
        }
    }
}

impl Argument {
    fn collect_references(&self, references: &mut dyn FnMut(&identity::Definition)) {
        match self {
            Self::Lifetime(lifetime) => lifetime.collect_references(references),
            Self::Type(ty) => ty.collect_references(references),
            Self::Const(value) => value.collect_references(references),
        }
    }
}

impl Const {
    fn collect_references(&self, references: &mut dyn FnMut(&identity::Definition)) {
        match self {
            Self::Literal { .. } => {}
            Self::Parameter { .. } => {}
            Self::Unevaluated {
                definition,
                arguments,
            } => {
                references(definition);
                arguments
                    .iter()
                    .for_each(|argument| argument.collect_references(references));
            }
        }
    }
}

impl Implementation {
    #[allow(
        clippy::too_many_arguments,
        reason = "the proof-bundle constructor requires every canonical implementation field"
    )]
    pub(crate) fn new(
        key: Box<str>,
        id: identity::Definition,
        relevance: Box<[identity::Definition]>,
        trait_ref: Trait,
        self_type: Type,
        safety: Safety,
        polarity: Polarity,
        const_impl: bool,
        default_impl: bool,
        generics: Generics,
        associated: Box<[identity::Definition]>,
        span: Span,
        origin: Origin,
    ) -> Self {
        Self {
            key,
            id,
            relevance,
            trait_ref,
            self_type,
            safety,
            polarity,
            const_impl,
            default_impl,
            generics,
            associated,
            span,
            origin,
        }
    }

    /// Whether two observations describe the same immutable rustc
    /// implementation. Relevance and demanded associated identities are
    /// intentionally excluded because independent fragments prove subsets of
    /// those two canonical sets.
    pub(crate) fn same_structure(&self, other: &Self) -> bool {
        self.key == other.key
            && self.id == other.id
            && self.trait_ref == other.trait_ref
            && self.self_type == other.self_type
            && self.safety == other.safety
            && self.polarity == other.polarity
            && self.const_impl == other.const_impl
            && self.default_impl == other.default_impl
            && self.generics == other.generics
            && self.span == other.span
            && self.origin == other.origin
    }

    /// Consume the set-valued evidence from another structurally identical
    /// observation into this canonical implementation owner.
    pub(crate) fn join_evidence<E>(
        &mut self,
        other: &mut Self,
        mut reserve: impl FnMut(&mut Vec<identity::Definition>, usize) -> Result<(), E>,
    ) -> Result<(), E> {
        let mut relevance = std::mem::take(&mut self.relevance).into_vec();
        let mut incoming_relevance = std::mem::take(&mut other.relevance).into_vec();
        reserve(&mut relevance, incoming_relevance.len())?;
        relevance.append(&mut incoming_relevance);
        relevance.sort_unstable();
        relevance.dedup();

        let mut associated = std::mem::take(&mut self.associated).into_vec();
        let mut incoming_associated = std::mem::take(&mut other.associated).into_vec();
        reserve(&mut associated, incoming_associated.len())?;
        associated.append(&mut incoming_associated);
        associated.sort_unstable();
        associated.dedup();

        self.relevance = relevance.into_boxed_slice();
        self.associated = associated.into_boxed_slice();
        Ok(())
    }

    /// The canonical rustc-derived implementation identity.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The compiler definition identity of the implementation itself.
    pub const fn id(&self) -> &identity::Definition {
        &self.id
    }

    /// Complete sorted semantic endpoints through which this implementation is relevant.
    pub fn relevance(&self) -> impl ExactSizeIterator<Item = &identity::Definition> {
        self.relevance.iter()
    }

    /// Inherent or exact trait-reference evidence.
    pub const fn trait_ref(&self) -> &Trait {
        &self.trait_ref
    }

    /// The exact implemented self type.
    pub const fn self_type(&self) -> &Type {
        &self.self_type
    }

    /// Safe or unsafe implementation evidence.
    pub const fn safety(&self) -> Safety {
        self.safety
    }

    /// Positive or negative implementation polarity.
    pub const fn polarity(&self) -> Polarity {
        self.polarity
    }

    /// Whether this is a const-trait implementation.
    pub const fn const_impl(&self) -> bool {
        self.const_impl
    }

    /// Whether specialization marks this implementation as default.
    pub const fn default_impl(&self) -> bool {
        self.default_impl
    }

    /// Exact implementation generics and predicates.
    pub const fn generics(&self) -> &Generics {
        &self.generics
    }

    /// Canonical associated definition identities.
    pub fn associated(&self) -> impl ExactSizeIterator<Item = &identity::Definition> {
        self.associated.iter()
    }

    pub(crate) const fn associated_slice(&self) -> &[identity::Definition] {
        &self.associated
    }

    pub(crate) fn retains_associated_owner(&self, definition: &Definition) -> bool {
        match definition.owner() {
            Owner::Implementation(owner) => owner == self.id(),
            Owner::Definition(owner) => {
                matches!(self.trait_ref(), Trait::Trait { id, .. } if id == owner)
            }
            Owner::Free => false,
        }
    }

    /// The compiler metadata diagnostic position.
    pub const fn span(&self) -> &Span {
        &self.span
    }

    /// Source, expansion, or compiler-synthesized implementation provenance.
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    pub(crate) fn for_each_reference(&self, references: &mut dyn FnMut(&identity::Definition)) {
        if let Trait::Trait { id, arguments } = &self.trait_ref {
            references(id);
            arguments
                .iter()
                .for_each(|argument| argument.collect_references(references));
        }
        self.self_type.collect_references(references);
        self.generics.collect_references(references);
        self.associated.iter().for_each(references);
    }
}

impl Binder {
    pub(crate) fn new(owner: identity::Definition, depth: u32, index: u32, name: Box<str>) -> Self {
        Self {
            owner,
            depth,
            index,
            name,
        }
    }
}
