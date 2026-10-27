//! Structured Rawr resolution errors.

use crate::{ImmutableString, Span};
use std::fmt;

/// Stable category for a resolution failure.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Category {
    /// A second item reused an existing declaration name.
    DuplicateItem,
    /// A type path in an item header could not be resolved.
    UnknownType,
    /// A compile-time import path is not present in static host metadata.
    UnknownImport,
    /// A compile-time import targets a non-namespace entry.
    InvalidImport,
    /// A registered host type has the wrong number of structural arguments.
    InvalidTypeArguments,
    /// A resolved item body has no executable lowering yet.
    LoweringUnavailable,
    /// A declared derive cannot be proven for its nominal type.
    InvalidDerive,
    /// A nominal declaration contains the same field or variant twice.
    DuplicateMember,
    /// A nominal field or enum variant does not exist.
    UnknownMember,
    /// A call names no function in scope.
    UnknownFunction,
    /// A path names no value in scope.
    UnknownVariable,
    /// A local is used after its value was moved out, as rustc's E0382.
    MovedValue,
    /// An access conflicts with a live borrow, as rustc's borrow checker
    /// reports it (E0106, E0373, E0499, E0500, E0501, E0502, E0503, E0505,
    /// E0506, E0515, E0524, E0597, E0621, E0716).
    BorrowConflict,
    /// A nominal constructor has the wrong field or payload shape.
    InvalidConstruction,
    /// A statically known expression type does not match its declaration.
    TypeMismatch,
    /// A nominal field write targets an immutable binding.
    ImmutableBinding,
    /// An inherent impl targets something other than a visible script type.
    InvalidImplOwner,
    /// An inherent method name is declared more than once for one owner.
    DuplicateMethod,
    /// A method call has the wrong receiver shape or argument arity.
    InvalidMethodCall,
    /// A registered host operation has the wrong grammar, arity, or types.
    InvalidHostOperation,
    /// A value with required usage was discarded as an expression statement.
    UnusedValue,
    /// `self` appears outside an inherent method.
    SelfOutsideImpl,
    /// A shared receiver is used for mutation.
    ImmutableReceiver,
    /// A method signature uses a type form deferred by the nominal evaluator.
    InvalidMethodSignature,
    /// A nominal operation requires a capability the type did not derive.
    MissingCapability,
    /// A nominal enum match contains an invalid, unreachable, or incomplete pattern set.
    InvalidMatch,
    /// Source combines nominal operations with syntax not lowered in this phase.
    UnsupportedNominalSyntax,
    /// Syntax Rust does not have, as rustc reports it.
    Syntax,
    /// The restricted application-root contract was violated.
    InvalidApplication,
    /// A local preview declaration cannot be normalized at compile time.
    InvalidPreview,
    /// A marked scripted-widget lifecycle is invalid.
    InvalidWidget,
}

impl fmt::Display for Category {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateItem => formatter.write_str("duplicate item"),
            Self::UnknownType => formatter.write_str("unknown type"),
            Self::UnknownImport => formatter.write_str("unknown import"),
            Self::InvalidImport => formatter.write_str("invalid import"),
            Self::InvalidTypeArguments => formatter.write_str("invalid type arguments"),
            Self::LoweringUnavailable => formatter.write_str("lowering unavailable"),
            Self::InvalidDerive => formatter.write_str("invalid derive"),
            Self::DuplicateMember => formatter.write_str("duplicate member"),
            Self::UnknownMember => formatter.write_str("unknown member"),
            Self::UnknownFunction => formatter.write_str("unknown function"),
            Self::UnknownVariable => formatter.write_str("unknown variable"),
            Self::MovedValue => formatter.write_str("use of moved value"),
            Self::BorrowConflict => formatter.write_str("borrow conflict"),
            Self::InvalidConstruction => formatter.write_str("invalid construction"),
            Self::TypeMismatch => formatter.write_str("type mismatch"),
            Self::ImmutableBinding => formatter.write_str("immutable binding"),
            Self::InvalidImplOwner => formatter.write_str("invalid impl owner"),
            Self::DuplicateMethod => formatter.write_str("duplicate method"),
            Self::InvalidMethodCall => formatter.write_str("invalid method call"),
            Self::InvalidHostOperation => formatter.write_str("invalid host operation"),
            Self::UnusedValue => formatter.write_str("unused required value"),
            Self::SelfOutsideImpl => formatter.write_str("self outside impl"),
            Self::ImmutableReceiver => formatter.write_str("immutable receiver"),
            Self::InvalidMethodSignature => formatter.write_str("invalid method signature"),
            Self::MissingCapability => formatter.write_str("missing capability"),
            Self::InvalidMatch => formatter.write_str("invalid match"),
            Self::UnsupportedNominalSyntax => formatter.write_str("unsupported nominal syntax"),
            Self::Syntax => formatter.write_str("syntax error"),
            Self::InvalidApplication => formatter.write_str("invalid application"),
            Self::InvalidPreview => formatter.write_str("invalid preview"),
            Self::InvalidWidget => formatter.write_str("invalid widget protocol"),
        }
    }
}

/// Failure to promote a parsed unit into an opaque resolved program.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Error {
    /// A declaration reused an item name from the same unit.
    DuplicateItem {
        /// Reused declaration name.
        name: ImmutableString,
        /// Primary span on the second declaration.
        declaration: Span,
        /// Secondary span on the first declaration.
        previous_declaration: Span,
    },
    /// A header referenced neither a script type, builtin, nor host type.
    UnknownType {
        /// Qualified type name as written.
        name: ImmutableString,
        /// Exact span occupied by the unresolved path.
        reference: Span,
    },
    /// A `use` item references no registered static module, type, or item.
    UnknownImport {
        /// Complete qualified path as written.
        path: ImmutableString,
        /// Exact unresolved path or member span.
        reference: Span,
    },
    /// A `use` group prefix is not a static namespace.
    InvalidImport {
        /// Complete qualified prefix as written.
        path: ImmutableString,
        /// Stable explanation of the invalid target.
        reason: ImmutableString,
        /// Exact prefix span.
        reference: Span,
        /// Related declaration that makes this path invalid, when any.
        declaration: Option<Span>,
    },
    /// A registered facade type is applied with the wrong structural arity.
    InvalidTypeArguments {
        /// Qualified or imported type name as written.
        ty: ImmutableString,
        /// Declared type-parameter count.
        expected: usize,
        /// Supplied non-lifetime argument count.
        actual: usize,
        /// Exact applied type span.
        reference: Span,
    },
    /// An item body passed header resolution but cannot yet become executable.
    LoweringUnavailable {
        /// Item name owning the body.
        item: ImmutableString,
        /// Exact body span rejected before evaluation.
        body: Span,
    },
    /// A derive name or its recursive requirements are invalid.
    InvalidDerive {
        /// Nominal type declaring the derive.
        owner: ImmutableString,
        /// Derive capability as written.
        capability: ImmutableString,
        /// Stable explanation of the failed requirement.
        reason: ImmutableString,
        /// Primary span on the derive argument.
        attribute: Span,
        /// Secondary span on the nominal declaration.
        declaration: Span,
    },
    /// A struct field or enum variant name is declared twice.
    DuplicateMember {
        /// Nominal type containing the duplicate.
        owner: ImmutableString,
        /// Reused field or variant name.
        member: ImmutableString,
        /// Primary span on the second name.
        declaration: Span,
        /// Secondary span on the first name.
        previous_declaration: Span,
    },
    /// A call names no function in scope.
    UnknownFunction {
        /// The name as written.
        name: ImmutableString,
        /// A library function of that name, to import: `iced::widget::center`.
        import: Option<ImmutableString>,
        /// Exact source reference.
        reference: Span,
    },
    /// A path names no local, constant or function in scope.
    UnknownVariable {
        /// The name as written.
        name: ImmutableString,
        /// Exact source reference.
        reference: Span,
    },
    /// A local is used after its value was moved out, as rustc's E0382.
    MovedValue {
        /// The local's name.
        name: ImmutableString,
        /// Where the value was moved.
        moved: Span,
        /// The use after the move.
        reference: Span,
        /// The move is in a loop's body, which goes on to its next
        /// iteration and moves the value again: rustc's "value moved here,
        /// in previous iteration of loop". `moved` and `reference` are both
        /// the move.
        previous_iteration: bool,
    },
    /// An access conflicts with a borrow that is still live, as rustc's
    /// borrow checker reports it.
    BorrowConflict {
        /// rustc's code: E0106, E0373, E0499, E0500, E0501, E0502, E0503,
        /// E0505, E0506, E0515, E0524, E0597, E0621 or E0716, or `lifetime`
        /// for rustc's "lifetime may not live long enough", which has none.
        code: &'static str,
        /// rustc's message.
        message: ImmutableString,
        /// The access.
        access: Span,
        /// The borrow it conflicts with.
        borrow: Span,
    },
    /// A struct field or enum variant reference cannot be resolved.
    UnknownMember {
        /// Nominal owner type.
        owner: ImmutableString,
        /// Missing field or variant.
        member: ImmutableString,
        /// Exact source reference.
        reference: Span,
    },
    /// A member used as the other kind: a field called like a method
    /// (rustc's E0599), or a method read like a field (rustc's E0615).
    MemberKind {
        /// Nominal owner type.
        owner: ImmutableString,
        /// The field or method.
        member: ImmutableString,
        /// Whether the member is a field, which the source called.
        field: bool,
        /// Exact source reference.
        reference: Span,
    },
    /// A nominal construction omits, duplicates, or miscounts members.
    InvalidConstruction {
        /// Nominal owner type.
        owner: ImmutableString,
        /// Concise shape failure.
        reason: ImmutableString,
        /// Constructor source span.
        construction: Span,
    },
    /// An expression's proven type differs from the declared target type.
    TypeMismatch {
        /// Required type name.
        expected: ImmutableString,
        /// Actual type name.
        actual: ImmutableString,
        /// Expression or assignment span.
        expression: Span,
    },
    /// Mutation targets a binding declared without `mut`.
    ImmutableBinding {
        /// Binding name.
        binding: ImmutableString,
        /// Primary mutation span.
        mutation: Span,
        /// Secondary binding declaration span.
        declaration: Span,
    },
    /// An impl owner is not a visible nominal script type.
    InvalidImplOwner {
        /// Owner name as written.
        owner: ImmutableString,
        /// Exact owner type reference.
        reference: Span,
    },
    /// An owner declares the same inherent method name twice.
    DuplicateMethod {
        /// Nominal owner type.
        owner: ImmutableString,
        /// Reused method name.
        method: ImmutableString,
        /// Primary span on the duplicate declaration.
        declaration: Span,
        /// Secondary span on the first declaration.
        previous_declaration: Span,
    },
    /// A statically resolved method call has an invalid shape.
    InvalidMethodCall {
        /// Nominal owner type.
        owner: ImmutableString,
        /// Resolved method name.
        method: ImmutableString,
        /// Concise failure reason.
        reason: ImmutableString,
        /// Exact call span.
        call: Span,
    },
    /// A static facade item cannot perform the requested source operation.
    InvalidHostOperation {
        /// Qualified or imported item name.
        item: ImmutableString,
        /// Concise failure reason.
        reason: ImmutableString,
        /// Exact source operation span.
        operation: Span,
    },
    /// A value marked as required was discarded by a terminated expression.
    UnusedValue {
        /// Required value type.
        value: ImmutableString,
        /// Exact discarded expression span.
        expression: Span,
    },
    /// `self` appears without an associated-method receiver.
    SelfOutsideImpl {
        /// Exact `self` reference.
        reference: Span,
    },
    /// A mutation is attempted through an `&self` receiver.
    ImmutableReceiver {
        /// Nominal owner type.
        owner: ImmutableString,
        /// Primary mutation span.
        mutation: Span,
        /// Secondary `&self` declaration span.
        receiver: Span,
    },
    /// A method signature contains a deferred type form.
    InvalidMethodSignature {
        /// Nominal owner type.
        owner: ImmutableString,
        /// Method name.
        method: ImmutableString,
        /// Concise unsupported-signature reason.
        reason: ImmutableString,
        /// Exact signature type span.
        signature: Span,
    },
    /// An operation requires a capability not proven by the owner type.
    MissingCapability {
        /// Nominal owner type.
        owner: ImmutableString,
        /// Required capability.
        capability: ImmutableString,
        /// Operation source span.
        operation: Span,
    },
    /// A statically-known nominal enum match has an invalid pattern set.
    InvalidMatch {
        /// Enum type being matched.
        owner: ImmutableString,
        /// Stable explanation of the pattern failure.
        reason: ImmutableString,
        /// Primary source span on the pattern or complete match.
        pattern: Span,
    },
    /// A nominal script reaches syntax deliberately deferred to a later step.
    UnsupportedNominalSyntax {
        /// Stable description of the unsupported construct.
        construct: ImmutableString,
        /// Exact source position that selected it.
        source: Span,
    },
    /// Syntax Rust does not have, as rustc reports it: its message, the
    /// label under the token and rustc's help.
    Syntax {
        /// The error, as rustc words it.
        message: ImmutableString,
        /// The label under the token.
        label: ImmutableString,
        /// rustc's help, when it gives one.
        help: Option<ImmutableString>,
        /// The token's span.
        at: Span,
    },
    /// Restricted application-root discovery failed.
    Application(crate::application::Error),
    /// Local preview metadata or one of its exact callback proofs is invalid.
    InvalidPreview {
        /// Stable preview subject or referenced item name.
        preview: ImmutableString,
        /// Concise explanation of the rejected proof.
        reason: ImmutableString,
        /// Primary preview attribute, option, or signature span.
        reference: Span,
        /// Related subject or callback declaration, when useful.
        declaration: Option<Span>,
    },
    /// A marked scripted-widget lifecycle cannot be promoted.
    InvalidWidget {
        /// Nominal widget or hook involved in the failure.
        widget: ImmutableString,
        /// Stable explanation of the rejected protocol proof.
        reason: ImmutableString,
        /// Primary marker, hook, or signature span.
        reference: Span,
        /// Related marker or hook declaration, when useful.
        declaration: Option<Span>,
    },
}

impl Error {
    /// Classify this resolution failure without parsing its message.
    #[must_use]
    pub const fn category(&self) -> Category {
        match self {
            Self::DuplicateItem { .. } => Category::DuplicateItem,
            Self::UnknownType { .. } => Category::UnknownType,
            Self::UnknownImport { .. } => Category::UnknownImport,
            Self::InvalidImport { .. } => Category::InvalidImport,
            Self::InvalidTypeArguments { .. } => Category::InvalidTypeArguments,
            Self::LoweringUnavailable { .. } => Category::LoweringUnavailable,
            Self::InvalidDerive { .. } => Category::InvalidDerive,
            Self::DuplicateMember { .. } => Category::DuplicateMember,
            Self::UnknownMember { .. } | Self::MemberKind { .. } => Category::UnknownMember,
            Self::UnknownFunction { .. } => Category::UnknownFunction,
            Self::UnknownVariable { .. } => Category::UnknownVariable,
            Self::MovedValue { .. } => Category::MovedValue,
            Self::BorrowConflict { .. } => Category::BorrowConflict,
            Self::InvalidConstruction { .. } => Category::InvalidConstruction,
            Self::TypeMismatch { .. } => Category::TypeMismatch,
            Self::ImmutableBinding { .. } => Category::ImmutableBinding,
            Self::InvalidImplOwner { .. } => Category::InvalidImplOwner,
            Self::DuplicateMethod { .. } => Category::DuplicateMethod,
            Self::InvalidMethodCall { .. } => Category::InvalidMethodCall,
            Self::InvalidHostOperation { .. } => Category::InvalidHostOperation,
            Self::UnusedValue { .. } => Category::UnusedValue,
            Self::SelfOutsideImpl { .. } => Category::SelfOutsideImpl,
            Self::ImmutableReceiver { .. } => Category::ImmutableReceiver,
            Self::InvalidMethodSignature { .. } => Category::InvalidMethodSignature,
            Self::MissingCapability { .. } => Category::MissingCapability,
            Self::InvalidMatch { .. } => Category::InvalidMatch,
            Self::UnsupportedNominalSyntax { .. } => Category::UnsupportedNominalSyntax,
            Self::Syntax { .. } => Category::Syntax,
            Self::Application(_) => Category::InvalidApplication,
            Self::InvalidPreview { .. } => Category::InvalidPreview,
            Self::InvalidWidget { .. } => Category::InvalidWidget,
        }
    }

    /// Return the declaration or type name involved in the failure.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::DuplicateItem { name, .. }
            | Self::UnknownType { name, .. }
            | Self::UnknownFunction { name, .. }
            | Self::UnknownVariable { name, .. }
            | Self::MovedValue { name, .. } => name.as_str(),
            Self::BorrowConflict { message, .. } => message.as_str(),
            Self::UnknownImport { path, .. } | Self::InvalidImport { path, .. } => path.as_str(),
            Self::InvalidTypeArguments { ty, .. } => ty.as_str(),
            Self::LoweringUnavailable { item, .. } => item.as_str(),
            Self::InvalidDerive { owner, .. } | Self::DuplicateMember { owner, .. } => {
                owner.as_str()
            }
            Self::UnknownMember { owner, .. }
            | Self::MemberKind { owner, .. }
            | Self::InvalidConstruction { owner, .. }
            | Self::MissingCapability { owner, .. }
            | Self::InvalidMatch { owner, .. } => owner.as_str(),
            Self::TypeMismatch { expected, .. } => expected.as_str(),
            Self::ImmutableBinding { binding, .. } => binding.as_str(),
            Self::InvalidImplOwner { owner, .. }
            | Self::DuplicateMethod { owner, .. }
            | Self::InvalidMethodCall { owner, .. }
            | Self::ImmutableReceiver { owner, .. }
            | Self::InvalidMethodSignature { owner, .. } => owner.as_str(),
            Self::InvalidHostOperation { item, .. } => item.as_str(),
            Self::UnusedValue { value, .. } => value.as_str(),
            Self::SelfOutsideImpl { .. } => "self",
            Self::UnsupportedNominalSyntax { construct, .. } => construct.as_str(),
            Self::Syntax { message, .. } => message.as_str(),
            Self::Application(_) => "application",
            Self::InvalidPreview { preview, .. } => preview.as_str(),
            Self::InvalidWidget { widget, .. } => widget.as_str(),
        }
    }

    /// Return the span callers should present as the primary diagnostic label.
    #[must_use]
    pub const fn primary_span(&self) -> Span {
        match self {
            Self::DuplicateItem { declaration, .. } => *declaration,
            Self::UnknownType { reference, .. } => *reference,
            Self::UnknownImport { reference, .. }
            | Self::InvalidImport { reference, .. }
            | Self::InvalidTypeArguments { reference, .. } => *reference,
            Self::LoweringUnavailable { body, .. } => *body,
            Self::InvalidDerive { attribute, .. } => *attribute,
            Self::DuplicateMember { declaration, .. } => *declaration,
            Self::UnknownMember { reference, .. } | Self::MemberKind { reference, .. } => {
                *reference
            }
            Self::UnknownFunction { reference, .. } => *reference,
            Self::UnknownVariable { reference, .. } => *reference,
            Self::MovedValue { reference, .. } => *reference,
            Self::BorrowConflict { access, .. } => *access,
            Self::InvalidConstruction { construction, .. } => *construction,
            Self::TypeMismatch { expression, .. } => *expression,
            Self::ImmutableBinding { mutation, .. } => *mutation,
            Self::InvalidImplOwner { reference, .. } => *reference,
            Self::DuplicateMethod { declaration, .. } => *declaration,
            Self::InvalidMethodCall { call, .. } => *call,
            Self::InvalidHostOperation { operation, .. } => *operation,
            Self::UnusedValue { expression, .. } => *expression,
            Self::SelfOutsideImpl { reference } => *reference,
            Self::ImmutableReceiver { mutation, .. } => *mutation,
            Self::InvalidMethodSignature { signature, .. } => *signature,
            Self::MissingCapability { operation, .. } => *operation,
            Self::InvalidMatch { pattern, .. } => *pattern,
            Self::UnsupportedNominalSyntax { source, .. } => *source,
            Self::Syntax { at, .. } => *at,
            Self::Application(error) => error.primary_span(),
            Self::InvalidPreview { reference, .. } => *reference,
            Self::InvalidWidget { reference, .. } => *reference,
        }
    }

    /// Return a related declaration span when one exists.
    #[must_use]
    pub const fn secondary_span(&self) -> Option<Span> {
        match self {
            Self::DuplicateItem {
                previous_declaration,
                ..
            } => Some(*previous_declaration),
            Self::InvalidDerive { declaration, .. } => Some(*declaration),
            Self::DuplicateMember {
                previous_declaration,
                ..
            } => Some(*previous_declaration),
            Self::ImmutableBinding { declaration, .. } => Some(*declaration),
            Self::MovedValue {
                previous_iteration: true,
                ..
            } => None,
            Self::MovedValue { moved, .. } => Some(*moved),
            Self::BorrowConflict { borrow, .. } => Some(*borrow),
            Self::DuplicateMethod {
                previous_declaration,
                ..
            } => Some(*previous_declaration),
            Self::ImmutableReceiver { receiver, .. } => Some(*receiver),
            Self::InvalidImport { declaration, .. } => *declaration,
            Self::UnknownType { .. }
            | Self::UnknownImport { .. }
            | Self::InvalidTypeArguments { .. }
            | Self::LoweringUnavailable { .. } => None,
            Self::UnknownMember { .. }
            | Self::MemberKind { .. }
            | Self::UnknownFunction { .. }
            | Self::UnknownVariable { .. }
            | Self::InvalidConstruction { .. }
            | Self::TypeMismatch { .. }
            | Self::MissingCapability { .. }
            | Self::InvalidMatch { .. }
            | Self::UnsupportedNominalSyntax { .. }
            | Self::Syntax { .. } => None,
            Self::Application(error) => error.secondary_span(),
            Self::InvalidPreview { declaration, .. } => *declaration,
            Self::InvalidWidget { declaration, .. } => *declaration,
            Self::InvalidImplOwner { .. }
            | Self::InvalidMethodCall { .. }
            | Self::InvalidHostOperation { .. }
            | Self::UnusedValue { .. }
            | Self::SelfOutsideImpl { .. }
            | Self::InvalidMethodSignature { .. } => None,
        }
    }
}

/// Names checked Rawr keeps under another spelling (`switch`, `debug`, ..)
/// read as Rust's in every message (`tokenizer::demangle_names`).
impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = Written(self).to_string();
        formatter.write_str(&crate::tokenizer::demangle_names(&text))
    }
}

struct Written<'a>(&'a Error);

impl fmt::Display for Written<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Error::DuplicateItem {
                name,
                declaration,
                previous_declaration,
            } => write!(
                formatter,
                "duplicate item `{name}` at {declaration}; first declared at {previous_declaration}"
            ),
            Error::UnknownType { name, reference } => {
                write!(formatter, "unknown type `{name}` at {reference}")
            }
            Error::UnknownImport { path, reference } => {
                write!(formatter, "unknown import `{path}` at {reference}")
            }
            Error::InvalidImport {
                path,
                reason,
                reference,
                ..
            } => write!(formatter, "invalid import `{path}` at {reference}: {reason}"),
            Error::InvalidTypeArguments {
                ty,
                expected,
                actual,
                reference,
            } => write!(
                formatter,
                "invalid type arguments for `{ty}` at {reference}: expected {expected}, found {actual}"
            ),
            Error::LoweringUnavailable { item, body } => {
                write!(
                    formatter,
                    "item body for `{item}` cannot be lowered yet at {body}"
                )
            }
            Error::InvalidDerive {
                owner,
                capability,
                reason,
                attribute,
                ..
            } => write!(
                formatter,
                "cannot derive `{capability}` for `{owner}` at {attribute}: {reason}"
            ),
            Error::DuplicateMember {
                owner,
                member,
                declaration,
                previous_declaration,
            } => write!(
                formatter,
                "duplicate member `{owner}::{member}` at {declaration}; first declared at {previous_declaration}"
            ),
            Error::UnknownMember {
                owner,
                member,
                reference,
            } => write!(formatter, "unknown member `{owner}::{member}` at {reference}"),
            Error::MemberKind {
                owner,
                member,
                field: true,
                reference,
            } => write!(formatter, "field `{owner}::{member}` called like a method at {reference}"),
            Error::MemberKind {
                owner,
                member,
                field: false,
                reference,
            } => write!(formatter, "method `{owner}::{member}` read like a field at {reference}"),
            Error::UnknownFunction {
                name, reference, ..
            } => write!(formatter, "unknown function `{name}` at {reference}"),
            Error::UnknownVariable { name, reference } => {
                write!(formatter, "unknown variable `{name}` at {reference}")
            }
            Error::MovedValue {
                name,
                reference,
                previous_iteration: true,
                ..
            } => write!(
                formatter,
                "use of moved value `{name}` at {reference}: value moved here, in previous \
                 iteration of loop"
            ),
            Error::MovedValue {
                name,
                moved,
                reference,
                ..
            } => write!(
                formatter,
                "use of moved value `{name}` at {reference}; it was already moved at {moved}"
            ),
            Error::BorrowConflict {
                code,
                message,
                access,
                ..
            } => write!(formatter, "{message} at {access} ({code})"),
            Error::InvalidConstruction {
                owner,
                reason,
                construction,
            } => write!(
                formatter,
                "invalid construction of `{owner}` at {construction}: {reason}"
            ),
            Error::TypeMismatch {
                expected,
                actual,
                expression,
            } => write!(
                formatter,
                "type mismatch at {expression}: expected `{expected}`, found `{actual}`"
            ),
            Error::ImmutableBinding {
                binding,
                mutation,
                declaration,
            } => write!(
                formatter,
                "cannot mutate immutable binding `{binding}` at {mutation}; declared at {declaration}"
            ),
            Error::InvalidImplOwner { owner, reference } => write!(
                formatter,
                "cannot define inherent methods for `{owner}` at {reference}; impl owners must be visible nominal script types"
            ),
            Error::DuplicateMethod {
                owner,
                method,
                declaration,
                previous_declaration,
            } => write!(
                formatter,
                "duplicate method `{owner}::{method}` at {declaration}; first declared at {previous_declaration}"
            ),
            Error::InvalidMethodCall {
                owner,
                method,
                reason,
                call,
            } => write!(
                formatter,
                "invalid call to `{owner}::{method}` at {call}: {reason}"
            ),
            Error::InvalidHostOperation {
                item,
                reason,
                operation,
            } => write!(
                formatter,
                "invalid host operation `{item}` at {operation}: {reason}"
            ),
            Error::UnusedValue { value, expression } => write!(
                formatter,
                "unused required value `{value}` at {expression}; return, bind, or explicitly consume it"
            ),
            Error::SelfOutsideImpl { reference } => {
                write!(formatter, "`self` is only available inside an impl method at {reference}")
            }
            Error::ImmutableReceiver {
                owner,
                mutation,
                receiver,
            } => write!(
                formatter,
                "cannot mutate `{owner}` through shared receiver `&self` at {mutation}; receiver declared at {receiver}"
            ),
            Error::InvalidMethodSignature {
                owner,
                method,
                reason,
                signature,
            } => write!(
                formatter,
                "invalid signature for `{owner}::{method}` at {signature}: {reason}"
            ),
            Error::MissingCapability {
                owner,
                capability,
                operation,
            } => write!(
                formatter,
                "`{owner}` does not provide `{capability}` at {operation}"
            ),
            Error::InvalidMatch {
                owner,
                reason,
                pattern,
            } => write!(
                formatter,
                "invalid match for `{owner}` at {pattern}: {reason}"
            ),
            Error::UnsupportedNominalSyntax { construct, source } => write!(
                formatter,
                "nominal operation uses unsupported syntax `{construct}` at {source}"
            ),
            Error::Syntax { message, .. } => formatter.write_str(message),
            Error::Application(error) => error.fmt(formatter),
            Error::InvalidPreview {
                preview,
                reason,
                reference,
                ..
            } => write!(
                formatter,
                "invalid preview `{preview}` at {reference}: {reason}"
            ),
            Error::InvalidWidget {
                widget,
                reason,
                reference,
                ..
            } => write!(
                formatter,
                "invalid widget protocol for `{widget}` at {reference}: {reason}"
            ),
        }
    }
}

impl From<crate::application::Error> for Error {
    fn from(error: crate::application::Error) -> Self {
        Self::Application(error)
    }
}

impl From<crate::preview::Error> for Error {
    fn from(error: crate::preview::Error) -> Self {
        match error {
            crate::preview::Error::Missing => Self::InvalidPreview {
                preview: "catalog".into(),
                reason: "program declares no `#[preview]` entries".into(),
                reference: crate::Span::NONE,
                declaration: None,
            },
            crate::preview::Error::Application(error) => Self::Application(error),
            crate::preview::Error::ApplicationSemanticMismatch => Self::InvalidPreview {
                preview: "catalog".into(),
                reason: "final application contract does not match sealed staging semantics".into(),
                reference: crate::Span::NONE,
                declaration: None,
            },
            crate::preview::Error::RuntimeType { role } => Self::InvalidPreview {
                preview: "catalog".into(),
                reason: format!("preview {role} has no exact runtime representation").into(),
                reference: crate::Span::NONE,
                declaration: None,
            },
        }
    }
}

impl From<Error> for crate::ParseError {
    fn from(error: Error) -> Self {
        let position = error.primary_span().start();

        Self(
            Box::new(crate::PERR::Resolution(error)),
            position,
            crate::types::Rendered::default(),
        )
    }
}

impl std::error::Error for Error {}
