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
    /// A path names no item of a module, as rustc's E0425.
    NotInModule,
    /// A call names no function in scope.
    UnknownFunction,
    /// A path names no value in scope.
    UnknownVariable,
    /// A macro call names no macro in scope.
    UnknownMacro,
    /// An operator whose operands no impl of its trait covers, in rustc's
    /// own words for it, as rustc's E0277.
    Unimplemented,
    /// A name used as another kind of item than the one it names, as
    /// rustc's E0423.
    WrongKind,
    /// A local is used after its value was moved out, as rustc's E0382.
    MovedValue,
    /// An access conflicts with a live borrow, as rustc's borrow checker
    /// reports it (E0106, E0373, E0499, E0500, E0501, E0502, E0503, E0505,
    /// E0506, E0515, E0524, E0597, E0621, E0716).
    BorrowConflict,
    /// A nominal constructor has the wrong field or payload shape.
    InvalidConstruction,
    /// An expression's type differs from the one expected, as rustc's
    /// E0308.
    TypeMismatch,
    /// An `if` with no `else` where a value other than `()` is expected, as
    /// rustc's E0317.
    IfWithoutElse,
    /// A `return;` in a function whose output isn't `()`, as rustc's E0069.
    ReturnWithoutValue,
    /// An assignment to a variable not declared `mut`, as rustc's E0384.
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
    /// A `break` or `continue` with no label in a `while` condition, as
    /// rustc's E0590.
    UnlabeledJumpInWhileCondition,
    /// A root `main` that is `async` with no runtime's entry, as rustc's
    /// E0752.
    AsyncMain,
    /// A path into std, core or alloc that rustc refuses: one std doesn't
    /// have (E0432, E0433, E0425), or an unstable item (E0658).
    StdPath,
    /// A std item rawr resolves but can't run yet (D712.1).
    NotYet,
    /// A generic parameter whose type nothing gives, as rustc's E0282.
    TypeAnnotationsNeeded,
    /// A call with more or fewer arguments than its callee takes, as
    /// rustc's E0061, or E0057 for a closure.
    ArgumentCount,
    /// An `==` or `!=` whose left operand's type has no `PartialEq` impl
    /// for the right operand's, as rustc's E0277.
    CannotCompare,
    /// A closure or function whose parameters differ from the ones its
    /// callable's signature gives, as rustc's E0631.
    ArgumentSignature,
    /// An item private to a module the use is outside of, as rustc's E0603
    /// or E0624.
    PrivateItem,
    /// An integer literal its type can't hold, rustc's deny-by-default
    /// `overflowing_literals` lint.
    OverflowingLiteral,
    /// A local used before it is given a value, as rustc's E0381.
    Uninitialized,
    /// A local whose type nothing settles where rustc needs it, as rustc's
    /// E0282 (or E0283 and E0284 for an operator).
    LocalTypeNeeded,
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
            Self::NotInModule => formatter.write_str("not in module"),
            Self::UnknownFunction => formatter.write_str("unknown function"),
            Self::UnknownVariable => formatter.write_str("unknown variable"),
            Self::UnknownMacro => formatter.write_str("unknown macro"),
            Self::Unimplemented => formatter.write_str("operator not implemented"),
            Self::WrongKind => formatter.write_str("wrong kind of item"),
            Self::MovedValue => formatter.write_str("use of moved value"),
            Self::BorrowConflict => formatter.write_str("borrow conflict"),
            Self::InvalidConstruction => formatter.write_str("invalid construction"),
            Self::TypeMismatch => formatter.write_str("mismatched types"),
            Self::IfWithoutElse => formatter.write_str("`if` without `else`"),
            Self::ReturnWithoutValue => formatter.write_str("`return;` without a value"),
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
            Self::UnlabeledJumpInWhileCondition => {
                formatter.write_str("unlabeled jump in a `while` condition")
            }
            Self::AsyncMain => formatter.write_str("async main"),
            Self::StdPath => formatter.write_str("std path"),
            Self::NotYet => formatter.write_str("not yet run"),
            Self::TypeAnnotationsNeeded => formatter.write_str("type annotations needed"),
            Self::ArgumentCount => formatter.write_str("wrong number of arguments"),
            Self::CannotCompare => formatter.write_str("can't compare"),
            Self::ArgumentSignature => formatter.write_str("type mismatch in arguments"),
            Self::PrivateItem => formatter.write_str("private item"),
            Self::OverflowingLiteral => formatter.write_str("literal out of range"),
            Self::Uninitialized => formatter.write_str("used binding isn't initialized"),
            Self::LocalTypeNeeded => formatter.write_str("type annotations needed"),
        }
    }
}

/// What a call calls, as rustc names it in E0061 and E0057.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Callee {
    /// A free function, or a value of a function's or `fn` pointer's type.
    Function,
    /// A function of a type or trait called by its path (`Type::new(..)`).
    AssociatedFunction,
    /// A method called on a receiver (`value.method(..)`).
    Method,
    /// A closure, or a value of a closure trait's type (E0057).
    Closure,
    /// A tuple struct's constructor.
    Struct,
    /// A tuple variant's constructor.
    Variant,
}

impl Callee {
    /// rustc's code: E0057 for a closure, E0061 otherwise.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Closure => "E0057",
            _ => "E0061",
        }
    }

    /// The noun rustc's message names the callee by.
    const fn noun(self) -> &'static str {
        match self {
            Self::Function | Self::AssociatedFunction | Self::Closure => "function",
            Self::Method => "method",
            Self::Struct => "struct",
            Self::Variant => "enum variant",
        }
    }

    /// rustc's note at the callee's definition: "function defined here".
    #[must_use]
    pub const fn defined_here(self) -> &'static str {
        match self {
            Self::Function => "function defined here",
            Self::AssociatedFunction => "associated function defined here",
            Self::Method => "method defined here",
            Self::Closure => "closure defined here",
            Self::Struct => "tuple struct defined here",
            Self::Variant => "tuple variant defined here",
        }
    }
}

/// Where the program defines a macro a call can't see, which rustc's
/// "cannot find macro" points out.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MacroScope {
    /// The program defines no macro of the name.
    Undefined,
    /// One of the name is defined later in the call's module: its name.
    Later(Span),
    /// One of the name is defined in a module whose macros the call can't
    /// see.
    Elsewhere,
}

/// What a private item is, as rustc names it in E0603 and E0624.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Private {
    /// A free function.
    Function,
    /// A struct.
    Struct,
    /// An enum.
    Enum,
    /// A module.
    Module,
    /// A trait.
    Trait,
    /// A type alias.
    TypeAlias,
    /// A `use` binding, which a path reaches only from inside its module.
    Import,
    /// A method of an inherent impl.
    Method,
    /// An associated function of an inherent impl, which takes no `self`.
    AssociatedFunction,
}

impl Private {
    /// rustc's code: E0624 for an inherent impl's function, E0603 for an
    /// item a path names.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Method | Self::AssociatedFunction => "E0624",
            _ => "E0603",
        }
    }

    /// rustc's label at the item's definition: "private method defined
    /// here".
    #[must_use]
    pub const fn defined_here(self) -> &'static str {
        match self {
            Self::Function => "private function defined here",
            Self::Struct => "private struct defined here",
            Self::Enum => "private enum defined here",
            Self::Module => "private module defined here",
            Self::Trait => "private trait defined here",
            Self::TypeAlias => "private type alias defined here",
            Self::Import => "private import defined here",
            Self::Method => "private method defined here",
            Self::AssociatedFunction => "private associated function defined here",
        }
    }

    /// The noun rustc's message names the item by.
    #[must_use]
    pub const fn noun(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Module => "module",
            Self::Trait => "trait",
            Self::TypeAlias => "type alias",
            Self::Import => "import",
            Self::Method => "method",
            Self::AssociatedFunction => "associated function",
        }
    }
}

/// rustc's E0061 and E0057 message: "this function takes 2 arguments but 1
/// argument was supplied".
pub(crate) struct ArgumentCount {
    pub(crate) callee: Callee,
    pub(crate) expected: usize,
    pub(crate) found: usize,
}

impl fmt::Display for ArgumentCount {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let plural = |count: usize| if count == 1 { "" } else { "s" };
        write!(
            formatter,
            "this {} takes {} argument{} but {} argument{} {} supplied",
            self.callee.noun(),
            self.expected,
            plural(self.expected),
            self.found,
            plural(self.found),
            if self.found == 1 { "was" } else { "were" },
        )
    }
}

/// Why a use after a move is one: the moved local's type isn't `Copy`.
/// rustc labels its declaration "move occurs because `x` has type `T`,
/// which does not implement the `Copy` trait".
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct MoveCause {
    /// The local's name where it is declared.
    pub declaration: Span,
    /// The local's type, as rustc writes it.
    pub ty: ImmutableString,
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
        /// Whether the name is a path's first segment, as `Type::item` and
        /// `module::item` write it, which rustc reports as E0433.
        path: bool,
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
    /// A struct field or enum variant name is declared twice, or a generic
    /// parameter name in one item's generics.
    DuplicateMember {
        /// Nominal type containing the duplicate, or empty for an item's
        /// generic parameters (rustc's E0403).
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
    /// A macro call names no macro in scope, which rustc reports without a
    /// code.
    UnknownMacro {
        /// The macro's name as written, without its `!`.
        name: ImmutableString,
        /// Exact source reference of the name.
        reference: Span,
        /// Where the program defines one of the name, if anywhere.
        scope: MacroScope,
    },
    /// An operator whose operands no impl of its trait covers, which rustc
    /// refuses (E0277) in its own words for the operator: a `?` in a
    /// function returning `()` or on an error with no `From` conversion to
    /// the output's, or arithmetic on two numeric types (`f64 / usize`).
    Unimplemented {
        /// rustc's message.
        message: ImmutableString,
        /// rustc's label under the operator.
        label: ImmutableString,
        /// rustc's help, when it gives one.
        help: Option<ImmutableString>,
        /// The operator.
        at: Span,
    },
    /// A name used as another kind of item than the one it names, which
    /// rustc refuses (E0423): a macro called as a function, a struct used
    /// as a value.
    WrongKind {
        /// What the use expects: `function`, `value`.
        expected: &'static str,
        /// What the name is: `macro`, `struct`.
        found: &'static str,
        /// The name.
        name: ImmutableString,
        /// The name's use.
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
        /// The use borrows the value, as `format!` and `&self` calls do:
        /// rustc's "borrow of moved value" rather than "use of".
        borrowed: bool,
        /// Why the value moves, where the lowering knows the local's type.
        cause: Option<MoveCause>,
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
        /// E0505, E0506, E0515, E0524, E0594, E0596, E0597, E0621 or E0716, or `lifetime`
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
    /// A path names no item of a module that exists, as rustc's E0425
    /// "cannot find function `load` in module `google`": an item the crate
    /// doesn't have, or one rawr has no binding for yet.
    NotInModule {
        /// The module's path as written.
        module: ImmutableString,
        /// The item's name.
        item: ImmutableString,
        /// Exact source reference of the item's name.
        reference: Span,
    },
    /// An item private to a module the use is outside of: rustc's E0603
    /// "function `f` is private" for a path, E0624 "method `get` is
    /// private" for an inherent impl's function.
    PrivateItem {
        /// What the item is.
        item: Private,
        /// The item's name.
        name: ImmutableString,
        /// Exact source reference of the item's name.
        reference: Span,
        /// The item's declaration, when it is in the same source.
        declaration: Option<Span>,
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
    /// An `if` with no `else` whose value is expected to have a type other
    /// than `()`, which rustc refuses (E0317).
    IfWithoutElse {
        /// The type expected.
        expected: ImmutableString,
        /// The `if` expression.
        expression: Span,
    },
    /// A `return;` in a function whose output isn't `()`, which rustc
    /// refuses (E0069).
    ReturnWithoutValue {
        /// The function's output type.
        output: ImmutableString,
        /// The `return` keyword.
        at: Span,
    },
    /// An assignment to a variable declared without `mut`, which rustc
    /// refuses (E0384).
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
        /// Nominal owner type, or empty for a free function or a closure.
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
    /// An imported method call that needs a proof-backed application its
    /// compilation does not hold yet. `call` is the encoded
    /// `rawr_import::surface::application::Call`.
    ImportedApplicationRequired {
        /// Qualified imported member name.
        item: ImmutableString,
        /// The encoded source call.
        call: Box<[u8]>,
        /// The ordinary miss this call is, when the facade has no member of
        /// its name: what is reported, and demanded, if no proof applies.
        miss: Option<Box<Error>>,
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
        /// Nominal owner type, or empty for a free function.
        owner: ImmutableString,
        /// Method name.
        method: ImmutableString,
        /// Concise unsupported-signature reason.
        reason: ImmutableString,
        /// Exact signature type span.
        signature: Span,
    },
    /// A type doesn't implement a trait an operation needs: rustc's E0277,
    /// "the trait bound `Owner: Trait` is not satisfied".
    MissingCapability {
        /// The type, as the program names it.
        owner: ImmutableString,
        /// The trait it must implement.
        capability: ImmutableString,
        /// Operation source span.
        operation: Span,
        /// Whether the type is the program's own struct or enum, so a
        /// `#[derive]` on it can provide a derivable trait.
        derive: bool,
        /// The item whose bound requires the trait, as rustc's "required by
        /// a bound in" note names it.
        required_by: Option<ImmutableString>,
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
    /// A `break` or `continue` with no label in a `while` condition, which
    /// rustc refuses (E0590).
    UnlabeledJumpInWhileCondition {
        /// `break` or `continue`.
        keyword: ImmutableString,
        /// The jump's span.
        at: Span,
    },
    /// A root `fn main` that is `async` with no runtime's entry, such as
    /// `#[tokio::main]`, which rustc refuses (E0752).
    AsyncMain {
        /// The function's signature, from `async` to its output.
        at: Span,
    },
    /// A path into std, core or alloc that rustc refuses, in rustc's words:
    /// one std doesn't have (E0432, E0433, E0425), or an unstable item
    /// (E0658). From the std table (D711).
    StdPath {
        /// rustc's code.
        code: &'static str,
        /// rustc's message.
        message: ImmutableString,
        /// rustc's label on the path, when it has one.
        label: Option<ImmutableString>,
        /// rustc's note, as E0658's issue link.
        note: Option<ImmutableString>,
        /// The path.
        reference: Span,
    },
    /// A std item rawr resolves but can't run yet, at its use (D712.1).
    NotYet {
        /// The item's path as the program names it: `std::sync::Mutex`.
        path: ImmutableString,
        /// The use.
        reference: Span,
    },
    /// A local used where it may not have a value yet, which rustc refuses
    /// (E0381): `let x: i32; println!("{x}");`.
    Uninitialized {
        /// The local.
        name: ImmutableString,
        /// Whether some path to the use gives it a value: rustc's
        /// "possibly-uninitialized".
        possibly: bool,
        /// The use.
        reference: Span,
        /// The local's declaration, which gives it no value.
        declaration: Span,
    },
    /// A local whose type nothing settles, or not where rustc needs it
    /// known, which rustc refuses: E0282, or E0283 and E0284 where an
    /// operator's trait needs it. `let v = Vec::new();` with nothing put in.
    LocalTypeNeeded {
        /// rustc's code.
        code: &'static str,
        /// The local.
        name: ImmutableString,
        /// The local's type as rustc writes it (`Vec<_>`), when part of it
        /// is known.
        ty: Option<ImmutableString>,
        /// The local's name where it is declared.
        declaration: Span,
        /// Where its type must be known, when rustc says.
        needed: Option<Span>,
        /// rustc's note on the trait the type must implement, as
        /// "the type must implement `From<u8>`".
        note: Option<ImmutableString>,
    },
    /// An integer literal its type can't hold, which rustc's
    /// deny-by-default `overflowing_literals` lint refuses: "literal out of
    /// range for `u8`".
    OverflowingLiteral {
        /// The literal's type: `u8`.
        ty: ImmutableString,
        /// rustc's note: the literal as written, and the type's range.
        note: ImmutableString,
        /// rustc's help, a type that holds the literal, when it has one.
        help: Option<ImmutableString>,
        /// The literal.
        literal: Span,
    },
    /// A call of a generic function that gives one of its type parameters
    /// no type, which rustc refuses (E0282).
    TypeAnnotationsNeeded {
        /// The type parameter as the function declares it.
        parameter: ImmutableString,
        /// The call.
        reference: Span,
    },
    /// A call with more or fewer arguments than its callee takes, which
    /// rustc refuses (E0061, or E0057 for a closure).
    ArgumentCount {
        /// What the call calls.
        callee: Callee,
        /// The callee as the program names it: `name` or `Owner::name`.
        name: ImmutableString,
        /// How many arguments the callee takes.
        expected: usize,
        /// How many the call gives.
        found: usize,
        /// The call, from the callee's name to its closing parenthesis.
        call: Span,
        /// The callee's name or header where the program defines it, when
        /// that is in the call's file.
        definition: Option<Span>,
    },
    /// `lhs == rhs` (or `!=`) where no `PartialEq<rhs>` impl holds for
    /// `lhs`, which rustc refuses (E0277).
    CannotCompare {
        /// The left type of the comparison no impl covers, as rustc names it.
        lhs: ImmutableString,
        /// The right type.
        rhs: ImmutableString,
        /// The operands' own types, when the comparison that fails is one
        /// an impl of theirs requires (`&&str == &str` needs `&str == str`).
        required: Option<(ImmutableString, ImmutableString)>,
        /// The operator.
        at: Span,
    },
    /// A closure or function passed where a callable of the same arity is
    /// expected whose parameters differ, which rustc refuses (E0631).
    ArgumentSignature {
        /// Whether a closure is passed; else a function.
        closure: bool,
        /// The signature expected, as `fn(&State) -> _`.
        expected: ImmutableString,
        /// The signature found, as `fn(State) -> _`.
        found: ImmutableString,
        /// The closure or function passed.
        at: Span,
    },
}

impl Error {
    /// rustc's E0061 (E0057 for a closure) for a call of `name` that gives
    /// `found` arguments where it takes `expected`. Its definition is not in
    /// the program's source.
    pub(crate) fn argument_count(
        callee: Callee,
        name: impl Into<ImmutableString>,
        expected: usize,
        found: usize,
        call: Span,
    ) -> Self {
        Self::ArgumentCount {
            callee,
            name: name.into(),
            expected,
            found,
            call,
            definition: None,
        }
    }

    /// rustc's E0596 for a `&mut` borrow of `binding`, a variable not
    /// declared `mut`, as a `&mut self` method or an `IndexMut` takes it.
    pub(crate) fn borrow_immutable(binding: &str, access: Span) -> Self {
        Self::BorrowConflict {
            code: "E0596",
            message: format!(
                "cannot borrow `{binding}` as mutable, as it is not declared as mutable"
            )
            .into(),
            access,
            borrow: Span::NONE,
        }
    }

    /// rustc's E0596 for a `&mut` borrow of `place`, a part of `binding`, a
    /// variable not declared `mut`.
    pub(crate) fn borrow_immutable_place(place: &str, binding: &str, access: Span) -> Self {
        if place == binding {
            return Self::borrow_immutable(binding, access);
        }
        Self::BorrowConflict {
            code: "E0596",
            message: format!(
                "cannot borrow `{place}` as mutable, as `{binding}` is not declared as mutable"
            )
            .into(),
            access,
            borrow: Span::NONE,
        }
    }

    /// rustc's E0594 for an assignment to `place`, a part of `binding`, a
    /// variable not declared `mut`.
    pub(crate) fn assign_immutable(place: &str, binding: &str, access: Span) -> Self {
        Self::BorrowConflict {
            code: "E0594",
            message: format!(
                "cannot assign to `{place}`, as `{binding}` is not declared as mutable"
            )
            .into(),
            access,
            borrow: Span::NONE,
        }
    }

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
            Self::NotInModule { .. } => Category::NotInModule,
            Self::UnknownFunction { .. } => Category::UnknownFunction,
            Self::UnknownVariable { .. } => Category::UnknownVariable,
            Self::UnknownMacro { .. } => Category::UnknownMacro,
            Self::WrongKind { .. } => Category::WrongKind,
            Self::Unimplemented { .. } => Category::Unimplemented,
            Self::MovedValue { .. } => Category::MovedValue,
            Self::BorrowConflict { .. } => Category::BorrowConflict,
            Self::InvalidConstruction { .. } => Category::InvalidConstruction,
            Self::TypeMismatch { .. } => Category::TypeMismatch,
            Self::IfWithoutElse { .. } => Category::IfWithoutElse,
            Self::ReturnWithoutValue { .. } => Category::ReturnWithoutValue,
            Self::ImmutableBinding { .. } => Category::ImmutableBinding,
            Self::InvalidImplOwner { .. } => Category::InvalidImplOwner,
            Self::DuplicateMethod { .. } => Category::DuplicateMethod,
            Self::InvalidMethodCall { .. } => Category::InvalidMethodCall,
            Self::InvalidHostOperation { .. } | Self::ImportedApplicationRequired { .. } => {
                Category::InvalidHostOperation
            }
            Self::UnusedValue { .. } => Category::UnusedValue,
            Self::SelfOutsideImpl { .. } => Category::SelfOutsideImpl,
            Self::ImmutableReceiver { .. } => Category::ImmutableReceiver,
            Self::InvalidMethodSignature { .. } => Category::InvalidMethodSignature,
            Self::MissingCapability { .. } => Category::MissingCapability,
            Self::InvalidMatch { .. } => Category::InvalidMatch,
            Self::UnsupportedNominalSyntax { .. } => Category::UnsupportedNominalSyntax,
            Self::Syntax { .. } => Category::Syntax,
            Self::UnlabeledJumpInWhileCondition { .. } => Category::UnlabeledJumpInWhileCondition,
            Self::AsyncMain { .. } => Category::AsyncMain,
            Self::StdPath { .. } => Category::StdPath,
            Self::NotYet { .. } => Category::NotYet,
            Self::TypeAnnotationsNeeded { .. } => Category::TypeAnnotationsNeeded,
            Self::ArgumentCount { .. } => Category::ArgumentCount,
            Self::CannotCompare { .. } => Category::CannotCompare,
            Self::ArgumentSignature { .. } => Category::ArgumentSignature,
            Self::PrivateItem { .. } => Category::PrivateItem,
            Self::OverflowingLiteral { .. } => Category::OverflowingLiteral,
            Self::Uninitialized { .. } => Category::Uninitialized,
            Self::LocalTypeNeeded { .. } => Category::LocalTypeNeeded,
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
            | Self::UnknownMacro { name, .. }
            | Self::WrongKind { name, .. }
            | Self::MovedValue { name, .. } => name.as_str(),
            Self::BorrowConflict { message, .. } => message.as_str(),
            Self::UnknownImport { path, .. } | Self::InvalidImport { path, .. } => path.as_str(),
            Self::InvalidTypeArguments { ty, .. } => ty.as_str(),
            Self::LoweringUnavailable { item, .. } => item.as_str(),
            Self::InvalidDerive { owner, .. } | Self::DuplicateMember { owner, .. } => {
                owner.as_str()
            }
            Self::NotInModule { module, .. } => module.as_str(),
            Self::PrivateItem { name, .. } => name.as_str(),
            Self::OverflowingLiteral { ty, .. } => ty.as_str(),
            Self::Uninitialized { name, .. } | Self::LocalTypeNeeded { name, .. } => name.as_str(),
            Self::UnknownMember { owner, .. }
            | Self::MemberKind { owner, .. }
            | Self::InvalidConstruction { owner, .. }
            | Self::MissingCapability { owner, .. }
            | Self::InvalidMatch { owner, .. } => owner.as_str(),
            Self::TypeMismatch { expected, .. } | Self::IfWithoutElse { expected, .. } => {
                expected.as_str()
            }
            Self::ReturnWithoutValue { output, .. } => output.as_str(),
            Self::ImmutableBinding { binding, .. } => binding.as_str(),
            Self::InvalidImplOwner { owner, .. }
            | Self::DuplicateMethod { owner, .. }
            | Self::InvalidMethodCall { owner, .. }
            | Self::ImmutableReceiver { owner, .. }
            | Self::InvalidMethodSignature { owner, .. } => owner.as_str(),
            Self::InvalidHostOperation { item, .. }
            | Self::ImportedApplicationRequired { item, .. } => item.as_str(),
            Self::UnusedValue { value, .. } => value.as_str(),
            Self::SelfOutsideImpl { .. } => "self",
            Self::UnsupportedNominalSyntax { construct, .. } => construct.as_str(),
            Self::Syntax { message, .. } | Self::Unimplemented { message, .. } => message.as_str(),
            Self::UnlabeledJumpInWhileCondition { keyword, .. } => keyword.as_str(),
            Self::AsyncMain { .. } => "main",
            Self::StdPath { message, .. } => message.as_str(),
            Self::NotYet { path, .. } => path.as_str(),
            Self::TypeAnnotationsNeeded { parameter, .. } => parameter.as_str(),
            Self::ArgumentCount { name, .. } => name.as_str(),
            Self::CannotCompare { lhs, .. } => lhs.as_str(),
            Self::ArgumentSignature { expected, .. } => expected.as_str(),
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
            Self::UnknownMember { reference, .. }
            | Self::MemberKind { reference, .. }
            | Self::NotInModule { reference, .. }
            | Self::PrivateItem { reference, .. } => *reference,
            Self::OverflowingLiteral { literal, .. } => *literal,
            Self::Uninitialized { reference, .. } => *reference,
            Self::LocalTypeNeeded { declaration, .. } => *declaration,
            Self::UnknownFunction { reference, .. } => *reference,
            Self::UnknownVariable { reference, .. }
            | Self::UnknownMacro { reference, .. }
            | Self::WrongKind { reference, .. } => *reference,
            Self::MovedValue { reference, .. } => *reference,
            Self::BorrowConflict { access, .. } => *access,
            Self::InvalidConstruction { construction, .. } => *construction,
            Self::TypeMismatch { expression, .. } | Self::IfWithoutElse { expression, .. } => {
                *expression
            }
            Self::ReturnWithoutValue { at, .. } => *at,
            Self::ImmutableBinding { mutation, .. } => *mutation,
            Self::InvalidImplOwner { reference, .. } => *reference,
            Self::DuplicateMethod { declaration, .. } => *declaration,
            Self::InvalidMethodCall { call, .. } => *call,
            Self::InvalidHostOperation { operation, .. }
            | Self::ImportedApplicationRequired { operation, .. } => *operation,
            Self::UnusedValue { expression, .. } => *expression,
            Self::SelfOutsideImpl { reference } => *reference,
            Self::ImmutableReceiver { mutation, .. } => *mutation,
            Self::InvalidMethodSignature { signature, .. } => *signature,
            Self::MissingCapability { operation, .. } => *operation,
            Self::InvalidMatch { pattern, .. } => *pattern,
            Self::UnsupportedNominalSyntax { source, .. } => *source,
            Self::AsyncMain { at } => *at,
            Self::StdPath { reference, .. } | Self::NotYet { reference, .. } => *reference,
            Self::TypeAnnotationsNeeded { reference, .. } => *reference,
            Self::ArgumentCount { call, .. } => *call,
            Self::Syntax { at, .. }
            | Self::Unimplemented { at, .. }
            | Self::UnlabeledJumpInWhileCondition { at, .. }
            | Self::CannotCompare { at, .. }
            | Self::ArgumentSignature { at, .. } => *at,
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
            Self::ImmutableBinding { declaration, .. }
            | Self::Uninitialized { declaration, .. } => Some(*declaration),
            Self::LocalTypeNeeded { needed, .. } => *needed,
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
            Self::InvalidImport { declaration, .. } | Self::PrivateItem { declaration, .. } => {
                *declaration
            }
            Self::ArgumentCount { definition, .. } => *definition,
            Self::UnknownMacro {
                scope: MacroScope::Later(definition),
                ..
            } => Some(*definition),
            Self::UnknownType { .. }
            | Self::UnknownImport { .. }
            | Self::InvalidTypeArguments { .. }
            | Self::LoweringUnavailable { .. } => None,
            Self::UnknownMember { .. }
            | Self::MemberKind { .. }
            | Self::NotInModule { .. }
            | Self::UnknownFunction { .. }
            | Self::UnknownVariable { .. }
            | Self::UnknownMacro { .. }
            | Self::WrongKind { .. }
            | Self::Unimplemented { .. }
            | Self::InvalidConstruction { .. }
            | Self::TypeMismatch { .. }
            | Self::IfWithoutElse { .. }
            | Self::ReturnWithoutValue { .. }
            | Self::MissingCapability { .. }
            | Self::InvalidMatch { .. }
            | Self::UnsupportedNominalSyntax { .. }
            | Self::Syntax { .. }
            | Self::AsyncMain { .. }
            | Self::StdPath { .. }
            | Self::NotYet { .. }
            | Self::TypeAnnotationsNeeded { .. }
            | Self::CannotCompare { .. }
            | Self::ArgumentSignature { .. }
            | Self::OverflowingLiteral { .. } => None,
            Self::UnlabeledJumpInWhileCondition { .. } => None,
            Self::InvalidImplOwner { .. }
            | Self::InvalidMethodCall { .. }
            | Self::InvalidHostOperation { .. }
            | Self::ImportedApplicationRequired { .. }
            | Self::UnusedValue { .. }
            | Self::SelfOutsideImpl { .. }
            | Self::InvalidMethodSignature { .. } => None,
        }
    }
}

/// rustc's message for a local whose type is unknown where it must be
/// known (E0282, E0283, E0284), with the type as far as it is known.
pub(crate) struct TypeNeeded<'a>(pub(crate) Option<&'a str>);

impl fmt::Display for TypeNeeded<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(ty) => write!(formatter, "type annotations needed for `{ty}`"),
            None => formatter.write_str("type annotations needed"),
        }
    }
}

/// rustc's message for a local used before it has a value (E0381).
pub(crate) struct Uninitialized<'a> {
    /// The local.
    pub(crate) name: &'a str,
    /// Whether some path to the use gives it a value.
    pub(crate) possibly: bool,
}

impl fmt::Display for Uninitialized<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = self.name;
        if self.possibly {
            write!(formatter, "used binding `{name}` is possibly-uninitialized")
        } else {
            write!(formatter, "used binding `{name}` isn't initialized")
        }
    }
}

/// rustc's message and label for an `async fn main` with no runtime's
/// entry (E0752).
pub const ASYNC_MAIN: &str = "`main` function is not allowed to be `async`";

/// rustc's message for an `if` with no `else` where a value is expected
/// (E0317).
pub const IF_WITHOUT_ELSE: &str = "`if` may be missing an `else` clause";

/// rustc's message for a `return;` in a function whose output isn't `()`
/// (E0069).
pub const RETURN_WITHOUT_VALUE: &str = "`return;` in a function whose return type is not `()`";

/// rustc's message for a name nothing in scope declares: E0425 for a value,
/// a function or a type, E0433 for a path's first segment, and the uncoded
/// "cannot find macro".
pub(crate) enum NotFound<'a> {
    Value(&'a str),
    Function(&'a str),
    Type(&'a str),
    Module(&'a str),
    Macro(&'a str),
}

impl<'a> NotFound<'a> {
    /// An unknown type's: a path's lowercase first segment names a module
    /// or a crate (`nothing::f`).
    pub(crate) fn type_named(name: &'a str, path: bool) -> Self {
        if path && name.starts_with(|first: char| first.is_lowercase() || first == '_') {
            Self::Module(name)
        } else {
            Self::Type(name)
        }
    }

    /// rustc's label for a path's unknown first segment (E0433).
    pub(crate) fn undeclared(&self) -> String {
        match self {
            Self::Module(name) => format!("use of unresolved module or unlinked crate `{name}`"),
            Self::Value(name) | Self::Function(name) | Self::Type(name) | Self::Macro(name) => {
                format!("use of undeclared type `{name}`")
            }
        }
    }
}

impl fmt::Display for NotFound<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (kind, name) = match self {
            Self::Value(name) => ("value", *name),
            Self::Function(name) => ("function", *name),
            Self::Type(name) => ("type", *name),
            Self::Module(name) => ("module or crate", *name),
            Self::Macro(name) => ("macro", *name),
        };
        // rustc names a path's last segment and the module it looks in.
        match name.rsplit_once("::") {
            Some((module, name)) => {
                write!(
                    formatter,
                    "cannot find {kind} `{name}` in module `{module}`"
                )
            }
            None => write!(formatter, "cannot find {kind} `{name}` in this scope"),
        }
    }
}

/// A type as rustc's "expected .., found .." labels name it: an integer
/// or a float literal whose type is still open by its kind, any other in
/// backticks.
pub(crate) struct Described<'a>(pub(crate) &'a str);

impl fmt::Display for Described<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            "{integer}" => formatter.write_str("integer"),
            "{float}" => formatter.write_str("floating-point number"),
            // The core's future is the opaque `impl Future` an async body
            // gives, which rustc calls a future.
            ty if ty
                .strip_prefix(crate::future::facade::FUTURE)
                .is_some_and(|rest| rest.starts_with('<')) =>
            {
                formatter.write_str("future")
            }
            ty => write!(formatter, "`{ty}`"),
        }
    }
}

/// rustc's E0384 message for an assignment to a variable not declared
/// `mut`.
pub(crate) struct AssignTwice<'a>(pub(crate) &'a str);

impl fmt::Display for AssignTwice<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "cannot assign twice to immutable variable `{}`",
            self.0
        )
    }
}

/// Names checked Rawr keeps under another spelling (`switch`, `debug`, ..)
/// read as Rust's in every message (`tokenizer::demangle_names`), and an
/// item reached through a crate's hidden segment reads from that crate's
/// root (`host::path::public`).
impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = Written(self).to_string();
        let text = crate::tokenizer::demangle_names(&text);
        formatter.write_str(&crate::host::path::public(&text))
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
            Error::UnknownType {
                name,
                reference,
                path,
            } => write!(
                formatter,
                "{} at {reference} ({})",
                NotFound::type_named(name, *path),
                if *path { "E0433" } else { "E0425" }
            ),
            Error::UnknownImport { path, reference } => {
                write!(formatter, "unresolved import `{path}` at {reference} (E0432)")
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
            } if owner.is_empty() => write!(
                formatter,
                "the name `{member}` is already used for a generic parameter at {declaration}; \
                 first used at {previous_declaration} (E0403)"
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
            Error::NotInModule {
                module,
                item,
                reference,
            } => write!(formatter, "cannot find `{item}` in module `{module}` at {reference}"),
            Error::PrivateItem {
                item,
                name,
                reference,
                ..
            } => write!(
                formatter,
                "{} `{name}` is private at {reference} ({})",
                item.noun(),
                item.code()
            ),
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
            } => write!(formatter, "{} at {reference} (E0425)", NotFound::Function(name)),
            Error::UnknownVariable { name, reference } => {
                write!(formatter, "{} at {reference} (E0425)", NotFound::Value(name))
            }
            Error::UnknownMacro {
                name, reference, ..
            } => {
                write!(formatter, "{} at {reference}", NotFound::Macro(name))
            }
            Error::Unimplemented { message, at, .. } => write!(formatter, "{message} at {at} (E0277)"),
            Error::WrongKind {
                expected,
                found,
                name,
                reference,
            } => write!(
                formatter,
                "expected {expected}, found {found} `{name}` at {reference} (E0423)"
            ),
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
                borrowed,
                ..
            } => write!(
                formatter,
                "{} of moved value `{name}` at {reference}; it was already moved at {moved}",
                if *borrowed { "borrow" } else { "use" }
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
                "mismatched types at {expression}: expected `{expected}`, found `{actual}` (E0308)"
            ),
            Error::IfWithoutElse {
                expected,
                expression,
            } => write!(
                formatter,
                "{IF_WITHOUT_ELSE} at {expression}: expected `{expected}`, found `()` (E0317)"
            ),
            Error::ReturnWithoutValue { output, at } => write!(
                formatter,
                "{RETURN_WITHOUT_VALUE} at {at}: expected `{output}` (E0069)"
            ),
            Error::ImmutableBinding {
                binding,
                mutation,
                declaration,
            } => write!(
                formatter,
                "{} at {mutation}; first assigned at {declaration} (E0384)",
                AssignTwice(binding)
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
                "invalid call to `{}` at {call}: {reason}",
                Qualified(owner, method)
            ),
            Error::InvalidHostOperation {
                item,
                reason,
                operation,
            } => write!(
                formatter,
                "invalid host operation `{item}` at {operation}: {reason}"
            ),
            Error::ImportedApplicationRequired {
                item, operation, ..
            } => write!(
                formatter,
                "`{item}` at {operation} needs a proof of which imported method applies in its scope"
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
                "invalid signature for `{}` at {signature}: {reason}",
                Qualified(owner, method)
            ),
            Error::MissingCapability {
                owner,
                capability,
                operation,
                ..
            } => write!(
                formatter,
                "{} at {operation} (E0277)",
                Unsatisfied(owner, capability)
            ),
            Error::InvalidMatch {
                owner,
                reason,
                pattern,
            } => write!(
                formatter,
                "invalid match for `{owner}` at {pattern}: {reason}"
            ),
            Error::UnsupportedNominalSyntax { construct, source } => {
                write!(formatter, "{} at {source}", Unsupported(construct))
            }
            Error::Syntax { message, .. } => formatter.write_str(message),
            Error::UnlabeledJumpInWhileCondition { .. } => formatter
                .write_str("`break` or `continue` with no label in the condition of a `while` loop"),
            Error::AsyncMain { .. } => formatter.write_str(ASYNC_MAIN),
            Error::StdPath {
                code,
                message,
                reference,
                ..
            } => write!(formatter, "{message} at {reference} ({code})"),
            Error::NotYet { path, reference } => {
                write!(formatter, "rawr doesn't run `{path}` yet at {reference}")
            }
            Error::Uninitialized {
                name,
                possibly,
                reference,
                ..
            } => write!(
                formatter,
                "{} at {reference} (E0381)",
                Uninitialized {
                    name,
                    possibly: *possibly
                }
            ),
            Error::LocalTypeNeeded {
                code,
                ty,
                declaration,
                note,
                ..
            } => {
                write!(formatter, "{} at {declaration}", TypeNeeded(ty.as_deref()))?;
                if let Some(note) = note {
                    write!(formatter, ": {note}")?;
                }
                write!(formatter, " ({code})")
            }
            Error::OverflowingLiteral { ty, literal, .. } => {
                write!(formatter, "literal out of range for `{ty}` at {literal}")
            }
            Error::TypeAnnotationsNeeded {
                parameter,
                reference,
            } => write!(
                formatter,
                "type annotations needed at {reference}: cannot infer the type of the type \
                 parameter `{parameter}` (E0282)"
            ),
            Error::ArgumentCount {
                callee,
                expected,
                found,
                call,
                ..
            } => write!(
                formatter,
                "{} at {call} ({})",
                ArgumentCount {
                    callee: *callee,
                    expected: *expected,
                    found: *found,
                },
                callee.code()
            ),
            Error::CannotCompare { lhs, rhs, at, .. } => write!(
                formatter,
                "{} at {at} (E0277)",
                CannotCompare(lhs, rhs)
            ),
            Error::ArgumentSignature {
                closure,
                expected,
                found,
                at,
            } => write!(
                formatter,
                "{} at {at}: expected {kind} signature `{expected}`, found {kind} signature \
                 `{found}` (E0631)",
                ArgumentSignature(*closure),
                kind = ArgumentSignature(*closure).kind(),
            ),
        }
    }
}

/// rustc's E0277 message for an `==` no `PartialEq` impl covers.
pub(crate) struct CannotCompare<'a>(pub(crate) &'a str, pub(crate) &'a str);

impl fmt::Display for CannotCompare<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "can't compare `{}` with `{}`", self.0, self.1)
    }
}

/// A construct rawr doesn't handle yet, named in Rust's terms. A reason
/// that already says it isn't supported reads as written.
pub(crate) struct Unsupported<'a>(pub(crate) &'a str);

impl fmt::Display for Unsupported<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.contains("not supported") {
            formatter.write_str(self.0)
        } else {
            write!(formatter, "{} is not supported", self.0)
        }
    }
}

/// rustc's E0631 message for a closure or function whose parameters
/// differ from the callable's it is passed as.
pub(crate) struct ArgumentSignature(pub(crate) bool);

impl ArgumentSignature {
    /// `closure` or `function`, as rustc's notes name it.
    pub(crate) const fn kind(&self) -> &'static str {
        if self.0 {
            "closure"
        } else {
            "function"
        }
    }
}

impl fmt::Display for ArgumentSignature {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "type mismatch in {} arguments", self.kind())
    }
}

/// rustc's E0277 message for a type that doesn't implement a trait: "the
/// trait bound `Owner: Trait` is not satisfied", or the formatting traits'
/// own wording.
pub(crate) struct Unsatisfied<'a>(pub(crate) &'a str, pub(crate) &'a str);

impl fmt::Display for Unsatisfied<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.1 {
            "Debug" => write!(formatter, "`{}` doesn't implement `Debug`", self.0),
            "Display" => write!(
                formatter,
                "`{}` doesn't implement `std::fmt::Display`",
                self.0
            ),
            "Send" => write!(
                formatter,
                "`{}` cannot be sent between threads safely",
                self.0
            ),
            "Sized" => write!(
                formatter,
                "the size for values of type `{}` cannot be known at compilation time",
                self.0
            ),
            // std's own wording for `collect()` (`#[rustc_on_unimplemented]`).
            capability
                if let Some(item) = capability
                    .strip_prefix("FromIterator<")
                    .and_then(|rest| rest.strip_suffix('>')) =>
            {
                write!(
                    formatter,
                    "a value of type `{}` cannot be built from an iterator over elements of type \
                     `{item}`",
                    self.0
                )
            }
            capability => write!(
                formatter,
                "the trait bound `{}: {capability}` is not satisfied",
                self.0
            ),
        }
    }
}

/// Whether `#[derive(capability)]` can provide a trait: the derives std
/// has.
pub(crate) fn derivable(capability: &str) -> bool {
    matches!(
        capability,
        "Clone" | "Copy" | "Debug" | "Default" | "PartialEq" | "Eq" | "PartialOrd" | "Ord" | "Hash"
    )
}

/// A member as the program names it: `Owner::member`, or `member` alone
/// for a free function or a closure, which have no owner.
pub(crate) struct Qualified<'a>(pub(crate) &'a str, pub(crate) &'a str);

impl fmt::Display for Qualified<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self("", member) => formatter.write_str(member),
            Self(owner, member) => write!(formatter, "{owner}::{member}"),
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
