//! Parsed Rawr item headers.

use crate::{capability, program, receiver, signature, type_def, ImmutableString, Span};
use std::fmt;

/// Parsed type references used by item headers.
pub mod type_ref;

/// The semantic kind of a declared Rawr item.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Kind {
    /// A nominal struct declaration.
    Struct,
    /// A nominal enum declaration.
    Enum,
    /// A free script function.
    Function,
    /// An inherent associated function without a receiver.
    AssociatedFunction,
    /// An inherent method with a receiver.
    Method,
    /// A nominal enum variant constructor.
    VariantConstructor,
}

/// A stable structural script item key.
///
/// Package, module path, optional owner, declared name, and semantic kind all
/// participate in identity. Dense resolver IDs never enter this key.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Key {
    path: program::Path,
    owner: Option<type_def::Identity>,
    kind: Kind,
}

impl fmt::Display for Key {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(type_def::Identity::Script(owner)) = &self.owner {
            write!(formatter, "{owner}::{}", self.name())
        } else {
            self.path.fmt(formatter)
        }
    }
}

impl Key {
    pub(crate) fn new(
        package: ImmutableString,
        modules: Box<[ImmutableString]>,
        name: ImmutableString,
        kind: Kind,
    ) -> Self {
        Self {
            path: program::Path::new(package, modules, name),
            owner: None,
            kind,
        }
    }

    pub(crate) fn associated(owner: &type_def::Key, name: ImmutableString, kind: Kind) -> Self {
        Self {
            path: program::Path::new(owner.package().into(), owner.modules().into(), name),
            owner: Some(type_def::Identity::Script(owner.clone())),
            kind,
        }
    }

    /// Return the canonical package component.
    #[must_use]
    pub fn package(&self) -> &str {
        self.path.package()
    }

    /// Return the canonical module-path components.
    #[must_use]
    pub fn modules(&self) -> &[ImmutableString] {
        self.path.modules()
    }

    /// Return the declared item name.
    #[must_use]
    pub fn name(&self) -> &str {
        self.path.name()
    }

    /// Return the stable owner identity for an associated item.
    #[must_use]
    pub const fn owner(&self) -> Option<&type_def::Identity> {
        self.owner.as_ref()
    }

    /// Return this item's semantic kind.
    #[must_use]
    pub const fn kind(&self) -> Kind {
        self.kind
    }

    /// This key with the declared name `name`, at the same owner and kind,
    /// as an instance of a generic method is keyed (D629).
    #[cfg(feature = "rust-import")]
    pub(crate) fn renamed(&self, name: ImmutableString) -> Self {
        Self {
            path: program::Path::new(
                self.package().into(),
                self.modules().to_vec().into_boxed_slice(),
                name,
            ),
            owner: self.owner.clone(),
            kind: self.kind,
        }
    }

    pub(crate) fn same_unowned_path(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

/// A persistent symbolic reference to executable Rawr intent.
///
/// Values contain only stable structural metadata. Invocation requires a
/// [`program::Authority`](crate::program::Authority) from a concrete compiled
/// generation.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Value {
    key: Key,
    kind: Kind,
    owner: Option<type_def::Identity>,
    receiver: Option<receiver::Mode>,
    signature: signature::Fingerprint,
    capabilities: capability::Fingerprint,
    bound: Option<Bound>,
}

/// The values a lifted closure captured where it was written, passed before
/// its own arguments whenever it is invoked. One capture is one value, equal
/// only to itself (and its clones): two closures are the same callable only
/// when they hold the same captures.
#[derive(Clone, Debug)]
struct Bound(crate::Shared<[crate::Dynamic]>);

impl Bound {
    fn address(&self) -> usize {
        crate::Shared::as_ptr(&self.0).cast::<crate::Dynamic>() as usize
    }
}

impl PartialEq for Bound {
    fn eq(&self, other: &Self) -> bool {
        crate::Shared::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Bound {}

impl PartialOrd for Bound {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Bound {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.address().cmp(&other.address())
    }
}

impl std::hash::Hash for Bound {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.address().hash(state);
    }
}

impl Value {
    #[cfg(feature = "rust-import")]
    pub(crate) fn normalize_profile(
        &mut self,
        profile: &crate::host::facade::ProfileFinalization<'_>,
    ) -> usize {
        self.signature.normalize_profile(profile)
    }

    pub(crate) fn new(
        key: Key,
        kind: Kind,
        owner: Option<type_def::Identity>,
        receiver: Option<receiver::Mode>,
        signature: signature::Fingerprint,
        capabilities: capability::Fingerprint,
    ) -> Self {
        Self {
            key,
            kind,
            owner,
            receiver,
            signature,
            capabilities,
            bound: None,
        }
    }

    /// This item with `captures` passed before its own arguments: a closure
    /// that captured them where it was written.
    pub(crate) fn bind(&self, captures: Vec<crate::Dynamic>) -> Self {
        Self {
            bound: (!captures.is_empty()).then(|| Bound(captures.into())),
            ..self.clone()
        }
    }

    /// The captured values passed before this item's own arguments.
    pub fn captures(&self) -> &[crate::Dynamic] {
        self.bound.as_ref().map_or(&[], |bound| &bound.0)
    }

    /// The same item as `other`, whatever each captured.
    #[must_use]
    pub fn same_item(&self, other: &Self) -> bool {
        self.key == other.key
            && self.kind == other.kind
            && self.owner == other.owner
            && self.receiver == other.receiver
            && self.signature == other.signature
            && self.capabilities == other.capabilities
    }

    /// `current`'s proof with this value's captures, after a rebind.
    pub(crate) fn rebound(&self, current: &Self) -> Self {
        Self {
            bound: self.bound.clone(),
            ..current.clone()
        }
    }

    /// Return the stable structural item key.
    #[must_use]
    pub const fn key(&self) -> &Key {
        &self.key
    }

    /// Return the semantic callable kind.
    #[must_use]
    pub const fn kind(&self) -> Kind {
        self.kind
    }

    /// Return the stable nominal owner, when this is an associated item.
    #[must_use]
    pub const fn owner(&self) -> Option<&type_def::Identity> {
        self.owner.as_ref()
    }

    /// Return the receiver mode for an inherent method.
    #[must_use]
    pub const fn receiver(&self) -> Option<receiver::Mode> {
        self.receiver
    }

    /// Return the exact canonical signature fingerprint.
    #[must_use]
    pub const fn signature(&self) -> &signature::Fingerprint {
        &self.signature
    }

    /// Return the exact callable capability fingerprint.
    #[must_use]
    pub const fn capabilities(&self) -> &capability::Fingerprint {
        &self.capabilities
    }
}

/// A generation-local resolved item identifier.
///
/// Values are created only while resolving a complete compilation unit.
/// Stable declaration identity uses [`Key`] instead.
///
/// ```compile_fail
/// let _ = rawr::item::Id(0);
/// ```
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Id(usize);

impl Id {
    pub(crate) const fn from_index(index: usize) -> Self {
        Self(index)
    }

    pub(crate) const fn index(self) -> usize {
        self.0
    }
}

/// A source name and its exact span.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Name {
    /// Name text.
    pub text: ImmutableString,
    /// Source span occupied by the name.
    pub span: Span,
}

/// Common metadata for an item or associated-function header.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Header {
    /// Attributes preceding the item.
    pub attributes: Box<[Attribute]>,
    /// Span of a `pub` visibility marker, when present.
    pub visibility: Option<Span>,
    /// Span from the first header token through its terminating delimiter.
    pub span: Span,
}

/// Whether `path` names a Rawr attribute: `preview`, or `rawr::preview`.
pub(crate) fn is_attribute_path(path: &type_ref::Path, name: &str) -> bool {
    match path.segments.as_ref() {
        [only] => only.name.text == name,
        [krate, only] => krate.name.text == "rawr" && only.name.text == name,
        _ => false,
    }
}

/// A parsed source attribute.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Attribute {
    /// Attribute path.
    pub path: type_ref::Path,
    /// Comma-separated path arguments inside parentheses.
    pub arguments: Box<[type_ref::Path]>,
    /// Complete `#[...]` source span.
    pub span: Span,
    pub(crate) preview: Option<crate::preview::Syntax>,
}

impl Attribute {
    pub(crate) const fn preview(&self) -> Option<&crate::preview::Syntax> {
        self.preview.as_ref()
    }

    pub(crate) fn is_widget(&self) -> bool {
        is_attribute_path(&self.path, "widget")
    }
}

/// A parsed top-level Rawr item.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Parsed {
    /// A file module declaration.
    Module(Module),
    /// A compile-time import.
    Use(Use),
    /// A nominal struct declaration.
    Struct(Struct),
    /// A nominal enum declaration.
    Enum(Enum),
    /// An inherent implementation block.
    Impl(Impl),
    /// A trait declaration.
    Trait(Trait),
    /// A top-level function with a Rawr header.
    Function(Function),
    /// A type alias: `type Name<T> = Type;`.
    TypeAlias(TypeAlias),
}

impl Parsed {
    /// Complete source span occupied by the item.
    #[must_use]
    pub const fn span(&self) -> Span {
        match self {
            Self::Module(item) => item.span,
            Self::Use(item) => item.span,
            Self::Struct(item) => item.span,
            Self::Enum(item) => item.span,
            Self::Impl(item) => item.span,
            Self::Trait(item) => item.span,
            Self::Function(item) => item.span,
            Self::TypeAlias(item) => item.span,
        }
    }
}

/// A type alias: `type Grid<T> = Vec<Vec<T>>;`. The resolver reads its
/// target once, in the module that declares it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeAlias {
    /// Alias header metadata.
    pub header: Header,
    /// Declared alias name.
    pub name: Name,
    /// The alias's generic parameters.
    pub generics: Box<[GenericParam]>,
    /// The aliased type.
    pub target: type_ref::Type,
    /// Complete item span, including the semicolon.
    pub span: Span,
}

/// A declared file module.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Module {
    /// Module declaration metadata.
    pub header: Header,
    /// Declared module name.
    pub name: Name,
    /// Complete item span, including the semicolon or the closing brace.
    pub span: Span,
    /// `mod name { .. }`: the span inside its braces, whose items are the
    /// module's, read as a file module of their own.
    pub inline: Option<Span>,
}

/// A compile-time import.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Use {
    /// Import header metadata.
    pub header: Header,
    /// Imported path or path group.
    pub tree: UseTree,
    /// Complete item span, including the semicolon.
    pub span: Span,
}

/// The supported shell forms of a compile-time import.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UseTree {
    /// One complete path, optionally renamed with `as`.
    Path(type_ref::Path, Option<Name>),
    /// A path prefix followed by a brace-delimited group.
    Group {
        /// Common path prefix.
        prefix: type_ref::Path,
        /// Names imported from the prefix.
        members: Box<[UseMember]>,
        /// Complete grouped path span.
        span: Span,
    },
    /// A glob import, `prefix::*`: every public name of a Rust module.
    Glob {
        /// The module path.
        prefix: type_ref::Path,
        /// Complete path span, including `*`.
        span: Span,
    },
}

/// One member of an import group: a name or `self`, optionally renamed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UseMember {
    /// The imported name; `self` imports the group prefix itself.
    pub name: Name,
    /// The local name given with `as`.
    pub alias: Option<Name>,
}

impl Use {
    /// Whether the import starts with `::`, which names an outside crate.
    #[must_use]
    pub fn global(&self) -> bool {
        match &self.tree {
            UseTree::Path(path, _)
            | UseTree::Group { prefix: path, .. }
            | UseTree::Glob { prefix: path, .. } => path.global,
        }
    }

    /// Whether `name` is bound by `self` in a group, `use a::m::{self}`,
    /// which imports only the module `m`, not a function of its name.
    #[must_use]
    pub fn binds_module_only(&self, name: &Name) -> bool {
        let UseTree::Group { members, .. } = &self.tree else {
            return false;
        };
        members.iter().any(|member| {
            member.name.text == "self"
                && member.alias.as_ref().unwrap_or(&member.name).span == name.span
        })
    }

    /// Each local name this import binds, with the full path it names.
    #[must_use]
    pub fn bindings(&self) -> Vec<(Name, Box<[ImmutableString]>)> {
        let segments = |path: &type_ref::Path| -> Vec<ImmutableString> {
            path.segments
                .iter()
                .map(|segment| segment.name.text.clone())
                .collect()
        };
        match &self.tree {
            UseTree::Path(path, alias) => {
                let name = path
                    .segments
                    .last()
                    .map(|segment| segment.name.clone())
                    .expect("parsed import paths retain one segment");
                vec![(alias.clone().unwrap_or(name), segments(path).into())]
            }
            UseTree::Group {
                prefix, members, ..
            } => members
                .iter()
                .map(|member| {
                    let mut path = segments(prefix);
                    let mut name = member.name.clone();
                    if member.name.text == "self" {
                        let last = prefix
                            .segments
                            .last()
                            .expect("parsed import paths retain one segment");
                        name.text = last.name.text.clone();
                    } else {
                        path.push(member.name.text.clone());
                    }
                    (member.alias.clone().unwrap_or(name), path.into())
                })
                .collect(),
            // A glob binds the names its module has, which only the resolver knows.
            UseTree::Glob { .. } => Vec::new(),
        }
    }
}

/// A generic type parameter: `T`, or `T: Clone + Debug`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GenericParam {
    /// Parameter name.
    pub name: Name,
    /// Trait bounds, from the parameter list and any `where` clause.
    pub bounds: Box<[type_ref::Path]>,
    /// Associated type equalities of its bounds: `Output = i64` of
    /// `T: Shape<Output = i64>`.
    /// Each is `(trait, name, type)`: `(Shape, Output, i64)`.
    pub projections: Box<[(Name, Name, type_ref::Type)]>,
    /// A default type: `Self` of `trait Add<Rhs = Self>`.
    pub default: Option<type_ref::Type>,
}

/// The lifetimes an item declares and the outlives bounds written on them,
/// which the borrow checker reads (D572.4).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Lifetimes {
    /// Declared lifetime parameters, in order: `'a` of `<'a, T>`.
    pub params: Box<[Name]>,
    /// Outlives bounds, from the parameter list and any `where` clause:
    /// `('b, 'a)` for `'b: 'a`, and `(T, 'a)` for `T: 'a`.
    pub bounds: Box<[(Name, Name)]>,
}

impl Lifetimes {
    pub(crate) fn new(params: Vec<Name>, bounds: Vec<(Name, Name)>) -> Self {
        Self {
            params: params.into_boxed_slice(),
            bounds: bounds.into_boxed_slice(),
        }
    }
}

/// A nominal struct declaration shell.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Struct {
    /// Struct header metadata.
    pub header: Header,
    /// Declared struct name.
    pub name: Name,
    /// Generic type parameters.
    pub generics: Box<[GenericParam]>,
    /// Lifetime parameters.
    pub lifetimes: Lifetimes,
    /// Named fields in declaration order; a tuple struct's are named
    /// `0`, `1`, ...
    pub fields: Box<[Field]>,
    /// How the fields are declared: `{ .. }`, `( .. )` or none (`;`).
    pub form: StructForm,
    /// Complete item span.
    pub span: Span,
}

/// How a struct declares its fields, which decides how it is built and
/// matched: `S { a: x }`, `S(x)`, or the unit value `S`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StructForm {
    /// `struct S { a: T }`, also `struct S {}`.
    Named,
    /// `struct S(T);`, also `struct S();`.
    Tuple,
    /// `struct S;`.
    Unit,
}

/// One named struct field.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Field {
    /// Field name.
    pub name: Name,
    /// Declared field type.
    pub ty: type_ref::Type,
    /// Complete field span.
    pub span: Span,
}

/// A nominal enum declaration shell.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Enum {
    /// Enum header metadata.
    pub header: Header,
    /// Declared enum name.
    pub name: Name,
    /// Generic type parameters.
    pub generics: Box<[GenericParam]>,
    /// Lifetime parameters.
    pub lifetimes: Lifetimes,
    /// Variants in declaration order.
    pub variants: Box<[Variant]>,
    /// Complete item span.
    pub span: Span,
}

/// One enum variant and its positional payload types.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Variant {
    /// Variant name.
    pub name: Name,
    /// Positional payload types.
    pub payload: Box<[type_ref::Type]>,
    /// The field names of a struct variant `V { x: T }`, one per payload
    /// type in declaration order.
    pub fields: Option<Box<[Name]>>,
    /// `#[default]`: the variant a derived `Default` builds.
    pub default: Option<Span>,
    /// `V = 5`: the explicit discriminant an `as` cast reads.
    pub discriminant: Option<crate::INT>,
    /// Complete variant span.
    pub span: Span,
}

/// An inherent or trait implementation block shell.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Impl {
    /// Impl header metadata.
    pub header: Header,
    /// Generic type parameters of `impl<T> ..`.
    pub generics: Box<[GenericParam]>,
    /// Lifetime parameters of `impl<'a> ..`.
    pub lifetimes: Lifetimes,
    /// The implemented trait of `impl Trait for Type`, when present.
    pub trait_: Option<type_ref::Path>,
    /// Type receiving the implementation.
    pub owner: type_ref::Type,
    /// Associated function headers.
    pub functions: Box<[Function]>,
    /// Associated types: `type Output = i64;`.
    pub associated: Box<[(Name, type_ref::Type)]>,
    /// Complete item span.
    pub span: Span,
}

/// A trait declaration: `trait Shape { fn area(&self) -> f64; .. }`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Trait {
    /// Trait header metadata.
    pub header: Header,
    /// Declared trait name.
    pub name: Name,
    /// Associated functions: required ones end in `;`, provided ones have a
    /// default body.
    pub functions: Box<[Function]>,
    /// Whether each function in `functions` has a default body.
    pub provided: Box<[bool]>,
    /// Associated types each impl names: `type Output;`.
    pub associated: Box<[Name]>,
    /// The bounds of each of `associated`: `Clone` of `type Output: Clone;`.
    pub associated_bounds: Box<[Box<[type_ref::Path]>]>,
    /// Supertraits: `Shape + Clone` of `trait Named: Shape + Clone`.
    pub supertraits: Box<[type_ref::Path]>,
    /// Type parameters: `T` of `trait Convert<T>`.
    pub generics: Box<[GenericParam]>,
    /// Complete item span.
    pub span: Span,
}

/// A function or associated-function header shell.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Function {
    /// Function header metadata.
    pub header: Header,
    /// Declared function name.
    pub name: Name,
    /// Generic type parameters.
    pub generics: Box<[GenericParam]>,
    /// Lifetime parameters.
    pub lifetimes: Lifetimes,
    /// The receiver's lifetime: `'a` of `&'a self`.
    pub receiver_lifetime: Option<Name>,
    /// Receiver for an associated function.
    pub receiver: Option<Receiver>,
    /// Named parameters after the receiver.
    pub parameters: Box<[Parameter]>,
    /// Declared return type, when present.
    pub output: Option<type_ref::Type>,
    /// Source span occupied by the body, including braces.
    pub body_span: Span,
    pub(crate) body_syntax: BodySyntax,
    /// `const NAME: T = value;`, lowered as a zero-argument function whose
    /// body span covers `value`; uses evaluate it, as Rust inlines it.
    pub(crate) constant: bool,
    /// `async fn`: the declared output is the future's, and the body runs
    /// as an `async move` block.
    pub(crate) asynchronous: bool,
    /// Complete item span.
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BodySyntax(Box<[BodyToken]>);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BodyToken {
    kind: BodyTokenKind,
    span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BodyTokenKind {
    Identifier(ImmutableString),
    DoubleColon,
    DoubleArrow,
    Match,
    LeftParen,
    RightParen,
    Unit,
    Comma,
    Dot,
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
    Semicolon,
    If,
    /// `||`: a closure of no parameters, as `iced::application(|| false, ..)`
    /// boots.
    OrOr,
    /// An integer literal, so a `const N: usize = 3;` has its value.
    Integer(crate::INT),
    /// `!`, as after a macro's name.
    Bang,
    Other,
}

impl BodySyntax {
    pub(crate) fn new(tokens: Vec<BodyToken>) -> Self {
        Self(tokens.into_boxed_slice())
    }

    pub(crate) fn tokens(&self) -> &[BodyToken] {
        &self.0
    }
}

impl BodySyntax {
    /// The value of a body that is one integer literal: of `const N: T =
    /// 3;`, as an array length names it.
    pub(crate) fn integer(&self) -> Option<crate::INT> {
        match self.tokens() {
            [token] => match token.kind() {
                BodyTokenKind::Integer(value) => Some(*value),
                _ => None,
            },
            _ => None,
        }
    }
}

impl BodyToken {
    pub(crate) const fn new(kind: BodyTokenKind, span: Span) -> Self {
        Self { kind, span }
    }

    pub(crate) const fn kind(&self) -> &BodyTokenKind {
        &self.kind
    }

    pub(crate) const fn span(&self) -> Span {
        self.span
    }
}

/// An associated function receiver.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Receiver {
    /// An `&self` receiver.
    Shared(Span),
    /// An `&mut self` receiver.
    Mutable(Span),
    /// A `self` receiver, which takes the value.
    Owned(Span),
    /// A `mut self` receiver, which takes the value into a mutable binding.
    OwnedMutable(Span),
}

impl Receiver {
    /// The receiver's source span.
    #[must_use]
    pub const fn span(&self) -> Span {
        match self {
            Self::Shared(span)
            | Self::Mutable(span)
            | Self::Owned(span)
            | Self::OwnedMutable(span) => *span,
        }
    }
}

/// One named function parameter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Parameter {
    /// Parameter name; a compiler-private name for a pattern parameter.
    pub name: Name,
    /// Declared parameter type.
    pub ty: type_ref::Type,
    /// Complete parameter span.
    pub span: Span,
    /// The pattern of a parameter written as one (`(x, y): (i64, i64)`,
    /// `&n: &i64`, `mut x: i64`, `_: i64`), bound at the top of the body.
    pub pattern: Option<ParameterPattern>,
}

/// An irrefutable parameter pattern.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParameterPattern {
    /// `name` or `mut name`.
    Binding {
        /// The bound name.
        name: Name,
        /// Whether the binding is `mut`.
        mutable: bool,
    },
    /// `_`.
    Discard(Span),
    /// `(a, b, ..)`.
    Tuple {
        /// Member patterns in order.
        members: Box<[ParameterPattern]>,
        /// Complete pattern span.
        span: Span,
    },
    /// `&p` or `&mut p`.
    Reference {
        /// Whether the reference is `&mut`.
        mutable: bool,
        /// The referenced pattern.
        pattern: Box<ParameterPattern>,
        /// Complete pattern span.
        span: Span,
    },
    /// `W(a, ..)` of a tuple struct, named by its path.
    TupleStruct {
        /// The struct's path segments.
        path: Box<[Name]>,
        /// Field patterns in order.
        members: Box<[ParameterPattern]>,
        /// Complete pattern span.
        span: Span,
    },
}

impl ParameterPattern {
    /// Complete source span occupied by the pattern.
    #[must_use]
    pub const fn span(&self) -> Span {
        match self {
            Self::Binding { name, .. } => name.span,
            Self::Discard(span)
            | Self::Tuple { span, .. }
            | Self::Reference { span, .. }
            | Self::TupleStruct { span, .. } => *span,
        }
    }
}
