//! Structured Rawr resolution errors.

use super::Imports;
use crate::{ImmutableString, Span};
use std::fmt;

/// Stable category for a resolution failure.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Category {
    /// A second item reused an existing declaration name.
    DuplicateItem,
    /// Two imports bound one name in one namespace (rustc's E0252).
    DuplicateImport,
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
    /// A nominal enum match contains an invalid or unreachable pattern set.
    InvalidMatch,
    /// A `match` whose arms miss some values of its scrutinee, as rustc's
    /// E0004.
    NonExhaustive,
    /// A `let`, `for` or parameter pattern that misses some values of what
    /// it binds, as rustc's E0005.
    Refutable,
    /// A tuple struct or tuple variant pattern with more or fewer fields
    /// than its type declares, as rustc's E0023.
    PatternFields,
    /// A name an or-pattern's alternative binds and another does not, as
    /// rustc's E0408.
    UnboundAlternative,
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
    /// `write!` into a value whose `Write` trait is not in scope, as
    /// rustc's E0599.
    UnscopedWrite,
    /// A method of a crate trait called where the trait is not in scope,
    /// as rustc's E0599.
    UnscopedMethod,
    /// An assignment to a value that is no place, as rustc's E0070 (E0067
    /// for a compound assignment).
    InvalidAssignee,
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
    /// A binary operator whose left operand's type has no impl of the
    /// operator's trait, as rustc's E0369 (E0368 for a compound
    /// assignment).
    BinaryOperation,
    /// A closure or function whose parameters differ from the ones its
    /// callable's signature gives, as rustc's E0631.
    ArgumentSignature,
    /// An item private to a module the use is outside of, as rustc's E0603
    /// or E0624.
    PrivateItem,
    /// An integer literal its type can't hold, rustc's deny-by-default
    /// `overflowing_literals` lint.
    OverflowingLiteral,
    /// An operation on values known when the body is compiled that would
    /// overflow or panic, rustc's deny-by-default `arithmetic_overflow` and
    /// `unconditional_panic` lints.
    KnownPanic,
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
            Self::DuplicateImport => formatter.write_str("duplicate import"),
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
            Self::NonExhaustive => formatter.write_str("non-exhaustive patterns"),
            Self::Refutable => formatter.write_str("refutable pattern"),
            Self::PatternFields => formatter.write_str("pattern field count"),
            Self::UnboundAlternative => formatter.write_str("variable not in all patterns"),
            Self::UnsupportedNominalSyntax => formatter.write_str("unsupported nominal syntax"),
            Self::Syntax => formatter.write_str("syntax error"),
            Self::UnlabeledJumpInWhileCondition => {
                formatter.write_str("unlabeled jump in a `while` condition")
            }
            Self::AsyncMain => formatter.write_str("async main"),
            Self::UnscopedWrite | Self::UnscopedMethod => formatter.write_str("trait not in scope"),
            Self::InvalidAssignee => formatter.write_str(INVALID_ASSIGNEE),
            Self::StdPath => formatter.write_str("std path"),
            Self::NotYet => formatter.write_str("not yet run"),
            Self::TypeAnnotationsNeeded => formatter.write_str("type annotations needed"),
            Self::ArgumentCount => formatter.write_str("wrong number of arguments"),
            Self::CannotCompare => formatter.write_str("can't compare"),
            Self::BinaryOperation => formatter.write_str("binary operation"),
            Self::ArgumentSignature => formatter.write_str("type mismatch in arguments"),
            Self::PrivateItem => formatter.write_str("private item"),
            Self::OverflowingLiteral => formatter.write_str("literal out of range"),
            Self::KnownPanic => formatter.write_str("operation will panic"),
            Self::Uninitialized => formatter.write_str("used binding isn't initialized"),
            Self::LocalTypeNeeded => formatter.write_str("type annotations needed"),
        }
    }
}

/// The deny-by-default lint that refuses an operation known to panic.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Lint {
    /// `arithmetic_overflow`: an operation that would overflow.
    ArithmeticOverflow,
    /// `unconditional_panic`: a division by zero, a division that would
    /// overflow, or an index past an array's end.
    UnconditionalPanic,
}

impl Lint {
    /// The lint's name, as `#[deny(..)]` names it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::ArithmeticOverflow => "arithmetic_overflow",
            Self::UnconditionalPanic => "unconditional_panic",
        }
    }

    /// rustc's message for the lint.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::ArithmeticOverflow => "this arithmetic operation will overflow",
            Self::UnconditionalPanic => "this operation will panic at runtime",
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
    /// A unit struct named as a value, its constructor.
    UnitStruct,
    /// An enum.
    Enum,
    /// A module.
    Module,
    /// A trait.
    Trait,
    /// A type alias.
    TypeAlias,
    /// A `use` binding of an item of the kind, which a path reaches only
    /// where its `pub` marker, or its lack of one, opens it to.
    Import(Imported),
    /// A method of an inherent impl.
    Method,
    /// An associated function of an inherent impl, which takes no `self`.
    AssociatedFunction,
    /// A tuple struct's constructor, private where one of its fields is.
    TupleStructConstructor,
}

/// The item a private import binds, which rustc's E0603 notes.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Referent {
    /// What the item is.
    pub item: Private,
    /// The item's name.
    pub name: ImmutableString,
    /// The item's name where it is declared.
    pub declaration: Span,
    /// Whether the item's own marker opens it to the use, which rustc
    /// says it could import directly.
    pub visible: bool,
    /// The path rustc suggests the use names instead.
    pub instead: Option<Instead>,
    /// The path as the use writes it, through the import's name.
    pub written: Span,
}

/// The path rustc's E0603 suggests for a private import.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Instead {
    /// The item's own path, when the use reaches the item by the import's
    /// name: `m::inner::f`, or `crate::m::inner::f` outside the crate's
    /// root.
    Item(ImmutableString),
    /// The import's own path with the import's name, otherwise:
    /// `self::inner::f` of `use self::inner::f;`.
    Import(ImmutableString),
}

/// What rustc's E0603 shows beside the private item.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Detail {
    /// A private import, and the item it binds, when that item is declared
    /// in the import's file.
    Import(Referent),
    /// A tuple struct's constructor, and its fields.
    Constructor(Fields),
    /// A private module on a path to an item past it.
    Through(Beyond),
}

/// A tuple struct's fields, where rustc's E0603 marks its private
/// constructor.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Fields {
    /// From the first field through the last, when one of them is
    /// private to the use: rustc's "a constructor is private if any of the
    /// fields is private".
    pub fields: Option<Span>,
    /// Where each field private to the use and with no `pub` marker
    /// starts, where rustc suggests writing `pub `.
    pub private: Box<[crate::Position]>,
    /// How many fields the struct declares, which rustc's help counts.
    pub count: usize,
}

/// The item a path names past a private module, which rustc's E0603
/// labels.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Beyond {
    /// What the item is.
    pub item: Private,
    /// The item's name.
    pub name: ImmutableString,
    /// The item's name where the path writes it.
    pub reference: Span,
    /// The path of a public re-export of the item the use sees, as rustc
    /// suggests it.
    pub reexport: Option<ImmutableString>,
    /// The path as the use writes it, through the item's name.
    pub written: Span,
}

/// What a private import binds, as rustc names it: "function import".
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Imported {
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
            Self::UnitStruct => "private unit struct defined here",
            Self::Enum => "private enum defined here",
            Self::Module => "private module defined here",
            Self::Trait => "private trait defined here",
            Self::TypeAlias => "private type alias defined here",
            Self::Import(_) => "private import defined here",
            Self::Method => "private method defined here",
            Self::AssociatedFunction => "private associated function defined here",
            Self::TupleStructConstructor => {
                "a constructor is private if any of the fields is private"
            }
        }
    }

    /// The noun rustc's message names the item by.
    #[must_use]
    pub const fn noun(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Struct => "struct",
            Self::UnitStruct => "unit struct",
            Self::Enum => "enum",
            Self::Module => "module",
            Self::Trait => "trait",
            Self::TypeAlias => "type alias",
            Self::Import(Imported::Function) => "function import",
            Self::Import(Imported::Struct) => "struct import",
            Self::Import(Imported::Enum) => "enum import",
            Self::Import(Imported::Module) => "module import",
            Self::Import(Imported::Trait) => "trait import",
            Self::Import(Imported::TypeAlias) => "type alias import",
            Self::Method => "method",
            Self::AssociatedFunction => "associated function",
            Self::TupleStructConstructor => "tuple struct constructor",
        }
    }

    /// The import of an item of this kind: `None` for what no `use`
    /// binds by itself, a method or a constructor.
    #[must_use]
    pub const fn imported(self) -> Option<Self> {
        let imported = match self {
            Self::Function => Imported::Function,
            Self::Struct | Self::UnitStruct => Imported::Struct,
            Self::Enum => Imported::Enum,
            Self::Module => Imported::Module,
            Self::Trait => Imported::Trait,
            Self::TypeAlias => Imported::TypeAlias,
            Self::Import(_)
            | Self::Method
            | Self::AssociatedFunction
            | Self::TupleStructConstructor => return None,
        };
        Some(Self::Import(imported))
    }
}

/// The item whose bound requires a trait, which rustc's E0277 notes:
/// "required by a bound in `f`".
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Required {
    /// The item, as rustc names it: `show`, `std::thread::spawn`.
    pub item: ImmutableString,
    /// Where the program writes the bound, and the call that needs it.
    pub place: Option<Box<Bound>>,
}

impl Required {
    /// A bound of `item`, which the program doesn't write.
    pub(crate) fn by(item: impl Into<ImmutableString>) -> Self {
        Self {
            item: item.into(),
            place: None,
        }
    }
}

/// The program's own enum a `match` misses variants of, as rustc's E0004
/// notes it at its definition.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Uncovered {
    /// The enum's name where it is declared.
    pub name: Span,
    /// The name of each variant no arm covers, where it is declared.
    pub variants: Box<[Span]>,
}

/// The placeholder of a format string whose argument's type lacks the trait
/// it formats with, as rustc's E0277 marks it.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Placeholder {
    /// The placeholder in the string, `{:?}`.
    pub at: Span,
    /// Whether it names its argument itself, `{s:?}`: rustc marks it alone.
    pub inline: bool,
}

impl Error {
    /// The error with the placeholder that formats its value, where it is
    /// the value's missing `Display` or `Debug` and `placed`, each format
    /// argument's span, trait and placeholder, has one for the value.
    #[must_use]
    pub(crate) fn formatted(mut self, placed: &[(Span, &str, Placeholder)]) -> Self {
        if let Self::MissingCapability {
            capability,
            operation,
            formatted: formatted @ None,
            ..
        } = &mut self
            && let Some((.., placeholder)) = placed
                .iter()
                .find(|(argument, name, _)| argument == operation && *name == capability.as_str())
        {
            *formatted = Some(Box::new(*placeholder));
        }
        self
    }
}

/// A bound the program writes on a function's generic parameter, and the
/// call whose argument doesn't meet it, as rustc's E0277 marks them.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Bound {
    /// The bound: `Display` of `T: Display`.
    pub bound: Span,
    /// The callee as the call names it.
    pub callee: Span,
    /// The argument whose type the parameter is.
    pub argument: Span,
}

/// A call's arguments and its callee's parameters where rustc's E0061
/// labels and suggests them: the arguments the callee doesn't take, and
/// the parameters the call gives nothing.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Arguments {
    /// The callee as the call names it: its path, or a method's name.
    pub callee: Span,
    /// Each argument the call gives, in order.
    pub given: Box<[Span]>,
    /// The arguments past what the callee takes.
    pub unexpected: Box<[Unexpected]>,
    /// The parameters the call gives nothing, in order.
    pub missing: Box<[Missing]>,
}

/// The value rustc's E0381 suggests a `let` write for its local, and
/// where: ` = 42` after `let x: i32`.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Assignment {
    /// After the local's name, or its type when the `let` writes one.
    pub at: Span,
    /// ` = ` and the value.
    pub code: ImmutableString,
}

/// The type rustc's E0282 suggests writing for a local, and where.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Annotation {
    /// After the local's name.
    pub at: Span,
    /// The type as far as it is known, each unknown part named by the
    /// parameter it fills (`Vec<T>`), or `/* Type */`.
    pub ty: ImmutableString,
    /// The first parameter an unknown part fills.
    pub parameter: Option<ImmutableString>,
}

/// An import of a name where rustc's E0252 shows it.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Import {
    /// Its path or group member, through any `as` rename.
    pub written: Span,
    /// Where an `as` rename of it goes, through the one written.
    pub rename: Span,
    /// What it binds, as rustc names it: `type`, `module`, `value` or
    /// `macro`.
    pub kind: &'static str,
}

/// A `match`'s arms as written, after which rustc's E0004 suggests the
/// arm that covers the values they miss.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Arms {
    /// Each arm, from its pattern to its value's end, before any `,`.
    pub arms: Box<[Span]>,
    /// The `match` from its scrutinee's end through its `}`.
    pub after_scrutinee: Span,
}

/// An argument past what a call's callee takes, as rustc's E0061 labels
/// it.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Unexpected {
    /// Its index among the call's arguments.
    pub index: usize,
    /// Its type as rustc names it.
    pub ty: ImmutableString,
}

/// A parameter a call gives no argument, as rustc's E0061 names it.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Missing {
    /// Its index among the callee's parameters, its receiver left out.
    pub index: usize,
    /// Its type as rustc names it: `None` for one inference leaves open.
    pub ty: Option<ImmutableString>,
    /// What rustc's suggestion writes in its place: `/* i32 */`, `()` for
    /// a unit, or `/* b */`, the parameter's name, for a type it can't
    /// write.
    pub placeholder: ImmutableString,
    /// The parameter where the program declares it, in the call's file.
    pub declared: Option<Span>,
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
/// A partial move's cause is rustc's note instead, "partial move occurs
/// because `p.a` has type `String`, ...", naming the part that moved.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct MoveCause {
    /// The local's name where it is declared, or for a partial move the
    /// part that moved, as rustc names it (`p.a`).
    pub moved: MovedPlace,
    /// The type of what moved, as rustc writes it.
    pub ty: ImmutableString,
    /// How rustc suggests keeping the value: cloning it at the move.
    pub cloning: Cloning,
    /// The parameter the move passes the value to, which rustc suggests
    /// making a borrow.
    pub parameter: Option<OwningParameter>,
}

/// What moved: a whole local, labelled where it is declared, or a part
/// of one, named in rustc's note.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum MovedPlace {
    /// The local's name where it is declared.
    Local(Span),
    /// The part, as rustc names it (`p.a`).
    Part(ImmutableString),
}

/// rustc's suggestion for keeping a moved value.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Cloning {
    /// The type is `Clone`: "consider cloning the value", with
    /// `.clone()` after the moved local.
    Clone,
    /// The type is the crate's own and not `Clone`: "if `T` implemented
    /// `Clone`, you could clone the value", noted at its definition.
    Derivable {
        /// The type, as rustc writes it.
        ty: ImmutableString,
        /// The type's name where it is defined, in the move's source.
        name: Span,
    },
    /// Neither: rustc suggests nothing.
    None,
}

/// A function's or method's parameter that takes ownership of a value
/// passed to it: rustc's note "consider changing this parameter type in
/// function `f` to borrow instead if owning the value isn't necessary".
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct OwningParameter {
    /// The function's name.
    pub function: ImmutableString,
    /// rustc's noun for it: `function` or `method`.
    pub kind: &'static str,
    /// The function's name where it is declared.
    pub name: Span,
    /// The parameter's type where it is written.
    pub ty: Span,
}

/// One program item whose crate derives need expanding (#593).
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Expansion {
    /// The source the item is written in.
    pub(crate) source: crate::program::source::Id,
    /// The item's path in its crate: its modules, then its name
    /// (`app::Task`).
    pub(crate) path: ImmutableString,
    /// The item, its attributes first.
    pub(crate) item: Span,
    /// Each crate derive: as written (`Serialize`), and the macro it
    /// names (`serde::Serialize`).
    pub(crate) derives: Box<[(ImmutableString, ImmutableString)]>,
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
    /// Two imports bound one name in one of Rust's namespaces, rustc's
    /// E0252: two functions, or a module and a type, of one name.
    DuplicateImport {
        /// The name both imports bind.
        name: ImmutableString,
        /// The namespace they bind it in: `type`, `value` or `macro`.
        namespace: &'static str,
        /// The second import, boxed with the first to keep the error small.
        declaration: Box<Import>,
        /// The first import.
        previous_declaration: Box<Import>,
    },
    /// A trait name nothing in scope declares: rustc's E0405 "cannot find
    /// trait `Display` in this scope".
    UnknownTrait {
        /// The name as written.
        name: ImmutableString,
        /// The name.
        reference: Span,
        /// The traits a `use` could bring in under the name.
        imports: Box<Imports>,
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
        /// The items a `use` could bring in under the name.
        imports: Box<Imports>,
    },
    /// A `use` item references no registered static module, type, or item.
    UnknownImport {
        /// Complete qualified path as written.
        path: ImmutableString,
        /// Exact unresolved path or member span.
        reference: Span,
        /// Where the path stops resolving, as rustc labels it.
        missing: Unresolved,
    },
    /// Several members of one `use` that each name nothing, which rustc
    /// reports as one E0432 with a label at each: built by
    /// [`Error::with_unknown_import`].
    UnknownImports {
        /// Each member's [`Error::UnknownImport`], in source order; two or
        /// more.
        imports: Box<[Error]>,
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
    /// A crate's derive on a program type whose expansion the program has
    /// not loaded (#593): rustc expands it, and checking the program again
    /// loads the expansion beside its item.
    ExpansionRequired {
        /// The first item's type, as written.
        owner: ImmutableString,
        /// The first item's crate derive, as written.
        attribute: Span,
        /// Every item a crate derive needs expanding for.
        items: Box<[Expansion]>,
    },
    /// A call of a crate's function-like macro whose expansion the program
    /// has not loaded (#1125): rustc expands it, and checking the program
    /// again reads the expansion in the call's place. `call` is the encoded
    /// `rawr_import::expand::call::Call`.
    MacroExpansionRequired {
        /// The macro's path from its crate's root (`rust_decimal_macros::dec`).
        path: ImmutableString,
        /// The encoded call.
        call: Box<[u8]>,
        /// Exact source span of the call's path.
        reference: Span,
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
        /// The items a `use` could bring in under the name: a library's
        /// function, `iced::widget::center`, or the program's
        /// `crate::m::f`.
        imports: Box<Imports>,
        /// Exact source reference.
        reference: Span,
    },
    /// A path names no local, constant or function in scope.
    UnknownVariable {
        /// The name as written.
        name: ImmutableString,
        /// The items a `use` could bring in under the name.
        imports: Box<Imports>,
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
        /// Only a part of the value moved: rustc's "partially moved".
        partial: bool,
        /// Why the value moves, where the lowering knows the local's type.
        /// Boxed: it is the largest part of any error, and most carry none.
        cause: Option<Box<MoveCause>>,
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
    /// A `&mut` borrow of a place that a local not declared `mut` owns,
    /// which rustc refuses (E0596 "cannot borrow `c` as mutable, as it is
    /// not declared as mutable"), suggesting `mut` where the local is
    /// declared.
    ImmutableBorrow {
        /// rustc's message.
        message: ImmutableString,
        /// The borrow.
        access: Span,
        /// The local's name where it is declared.
        declared: Span,
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
    /// `value.member(..)` or `Type::member` where the program's own
    /// struct or enum, or a reference to one, has no item of that name,
    /// which rustc refuses (E0599) naming the type's kind and marking its
    /// definition.
    NoItem {
        /// Nominal owner type, as [`Self::UnknownMember`] names it.
        owner: ImmutableString,
        /// The missing method or associated item.
        member: ImmutableString,
        /// The member's name in the source.
        reference: Span,
        /// The type as rustc names it.
        receiver: Box<Receiver>,
        /// An item of the type whose name is like `member`, which rustc
        /// suggests.
        similar: Option<Box<Similar>>,
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
        /// What rustc shows beside it: the item a private import binds,
        /// a constructor's fields, or the item a private module's path
        /// goes on to.
        detail: Option<Box<Detail>>,
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
        /// A called field's parenthesized arguments, which rustc suggests
        /// removing.
        arguments: Option<Span>,
        /// The program's own struct whose field is called, as rustc names
        /// it.
        receiver: Option<Box<Receiver>>,
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
    /// A value that is no box where a `Box<dyn Error>` is expected, which
    /// rustc refuses (E0308): only a box unsizes to a trait object's box.
    Unboxed {
        /// The box's type name.
        expected: ImmutableString,
        /// The value's type name.
        actual: ImmutableString,
        /// How rustc says to make the box: `Box::new` of an error, or
        /// `.into()` of text.
        help: Option<Boxing>,
        /// The value.
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
        /// Where the function writes its output type.
        written: Option<Span>,
        /// The value rustc suggests the `return` give, as `42`.
        value: Option<ImmutableString>,
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
        /// The binding's name where it is declared, where rustc suggests
        /// `mut`.
        declared: Span,
        /// Whether a pattern matched against a value declares it, where
        /// rustc also suggests `ref mut`, which assigns through to the
        /// value.
        matched: bool,
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
        /// Whether `self` is a parameter of a function outside an `impl`
        /// or a `trait`, rather than a value read where no receiver is.
        parameter: bool,
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
        /// The type's name where the program declares it, when that is in
        /// the file the error is in: where rustc suggests the `#[derive]`
        /// and shows `Display`'s help.
        declared: Option<Span>,
        /// The item whose bound requires the trait, as rustc's "required by
        /// a bound in" note names it.
        required_by: Option<Required>,
        /// The format string's placeholder that formats the value with the
        /// trait.
        formatted: Option<Box<Placeholder>>,
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
    /// A `match` whose arms miss some values of its scrutinee, which rustc
    /// refuses (E0004).
    NonExhaustive {
        /// The patterns of the values no arm matches, as rustc writes them.
        witnesses: Box<[ImmutableString]>,
        /// The matched value's type.
        ty: ImmutableString,
        /// The scrutinee.
        scrutinee: Span,
        /// The arms, where rustc suggests one more.
        arms: Box<Arms>,
        /// The program's own enum whose variants the arms miss, where the
        /// error's file declares it.
        uncovered: Option<Box<Uncovered>>,
        /// The `#[non_exhaustive]` enums whose `_` the first witness holds,
        /// by name, each of which rustc notes needs a wildcard.
        sealed: Box<[ImmutableString]>,
    },
    /// A pattern that must match every value, in a `let` without `else`, a
    /// `for` or a parameter, which misses some, which rustc refuses
    /// (E0005).
    Refutable {
        /// The patterns of the values it misses, as rustc writes them.
        witnesses: Box<[ImmutableString]>,
        /// The matched value's type.
        ty: ImmutableString,
        /// Where the pattern is.
        site: BindingSite,
        /// The pattern.
        pattern: Span,
    },
    /// A tuple struct or tuple variant pattern with more or fewer fields
    /// than its type declares, which rustc refuses (E0023).
    PatternFields {
        /// The struct or variant the pattern names, as written.
        owner: ImmutableString,
        /// Whether the pattern names a tuple variant, not a tuple struct.
        variant: bool,
        /// How many fields the struct or variant declares.
        expected: usize,
        /// How many field patterns the pattern writes, besides `..`.
        found: usize,
        /// The field patterns, or the whole pattern when it writes none.
        pattern: Span,
        /// Each declared field, when the program declares them in the
        /// pattern's file.
        fields: Box<[Span]>,
        /// rustc's help, with the pattern rewritten: `_` for each field it
        /// misses, or parentheses around its fields where the one declared
        /// field is a tuple of that many.
        help: Option<ImmutableString>,
    },
    /// A name one alternative of an or-pattern binds and another does not,
    /// which rustc refuses (E0408).
    UnboundAlternative {
        /// The name.
        name: ImmutableString,
        /// The alternative that does not bind it.
        pattern: Span,
        /// Where another alternative binds it.
        binding: Span,
    },
    /// An array pattern of more or fewer elements than its array has,
    /// which rustc refuses (E0527), or of more with a `..` (E0528).
    ArrayPatternLength {
        /// The matched array's type name.
        owner: ImmutableString,
        /// How many elements the pattern has, besides `..`.
        fixed: usize,
        /// How many elements the array has.
        length: usize,
        /// Whether the pattern has a `..`.
        rest: bool,
        /// The pattern.
        pattern: Span,
    },
    /// A pattern of another type than the value it matches, or a tuple
    /// pattern of more or fewer elements than its tuple has, which rustc
    /// refuses (E0308).
    PatternMismatch {
        /// The matched type and the pattern's.
        types: Box<PatternTypes>,
        /// The pattern.
        pattern: Span,
        /// What gave the matched value its type, which the statement or
        /// arm the pattern heads adds.
        origin: Option<PatternOrigin>,
    },
    /// A slice pattern on a value that is no array or slice, which rustc
    /// refuses (E0529).
    SlicePattern {
        /// The matched type's name.
        ty: ImmutableString,
        /// Whether the value is a `Vec`, which a slicing `[..]` makes a
        /// slice.
        sliceable: bool,
        /// The pattern.
        pattern: Span,
        /// The matched expression, when the pattern is its statement's or
        /// arm's whole pattern: where rustc's slicing `[..]` goes.
        subject: Option<Span>,
    },
    /// A second `..` in one tuple or slice pattern, which rustc refuses
    /// without a code.
    RepeatedRest {
        /// Whether the pattern is a slice pattern, not a tuple's.
        slice: bool,
        /// The first `..`.
        previous: Span,
        /// The second `..`.
        rest: Span,
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
    /// `write!(s, ..)` into a value whose `Write` impl's trait is not in
    /// scope where the macro is, which rustc refuses (E0599 "cannot write
    /// into `String`"): a trait's methods are callable only where the
    /// trait is in scope.
    UnscopedWrite {
        /// The value's type, as rustc writes it.
        owner: ImmutableString,
        /// The value written into.
        at: Span,
        /// Where rustc puts the `use` of `Write` it suggests.
        placement: Option<crate::Position>,
    },
    /// `value.member(..)` where only a crate trait out of scope has
    /// `member` for the value's type, which rustc refuses (E0599 "no
    /// method named `t` found for type `i32` in the current scope"): a
    /// trait's methods are callable only where the trait is in scope.
    UnscopedMethod {
        /// What rustc calls the receiver's type: `type`, `struct`, `enum`
        /// or `reference`.
        kind: &'static str,
        /// The receiver's type, as rustc writes it.
        owner: ImmutableString,
        /// The method.
        member: ImmutableString,
        /// The trait's path rustc suggests importing, `crate::m::T`.
        path: ImmutableString,
        /// The method's name in the call.
        at: Span,
        /// Where rustc puts the `use` it suggests.
        placement: Option<crate::Position>,
    },
    /// An assignment whose left side is a value, not a place, as
    /// `(x + y) = 42` or `f(x) += 1`, which rustc refuses (E0070, E0067 for
    /// a compound assignment).
    InvalidAssignee {
        /// Whether the assignment is a compound one, as `+=`.
        compound: bool,
        /// The assignment's operator.
        operator: Span,
        /// The left side.
        assignee: Span,
    },
    /// A path into std, core or alloc that rustc refuses, in rustc's words:
    /// one std doesn't have (E0432, E0433, E0425), or an unstable item
    /// (E0658). From the std table (D711). Another error that is a message
    /// and a label at one place, as rustc words it, is one too: a private
    /// field's (E0616, E0451) or a visibility's path (E0742).
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
        /// The value rustc suggests its `let` write, for a local its use
        /// finds with no value on any path, declared alone.
        assign: Option<Assignment>,
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
        /// The type rustc suggests writing for the local, when its `let`
        /// declares it alone with no type.
        annotation: Option<Box<Annotation>>,
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
    /// An operation on values known when the body is compiled that would
    /// overflow, divide by zero or index past an array's end, which rustc's
    /// deny-by-default `arithmetic_overflow` and `unconditional_panic` lints
    /// refuse.
    KnownPanic {
        /// The lint that refuses it.
        lint: Lint,
        /// rustc's label: what the operation attempts, as "attempt to
        /// compute `u8::MAX + 1_u8`, which would overflow".
        label: ImmutableString,
        /// The operation.
        at: Span,
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
        /// The arguments and parameters rustc labels, where the call
        /// knows them.
        arguments: Option<Box<Arguments>>,
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
    /// A binary operator whose left operand's type has no impl of the
    /// operator's trait, which rustc refuses (E0369, E0368 for a compound
    /// assignment).
    BinaryOperation {
        /// rustc's message and what its labels and note name.
        operation: Box<BinaryOperation>,
        /// The operator; a compound assignment's whole expression.
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
    /// This error, the unresolved members of one `use` so far, with
    /// `import`, another member's [`Error::UnknownImport`]: rustc's one
    /// E0432 for them all, its members in source order.
    pub(crate) fn with_unknown_import(self, import: Self) -> Self {
        debug_assert!(
            matches!(import, Self::UnknownImport { .. }),
            "a `use` gathers its members' unresolved imports alone"
        );
        // Members past one missing prefix miss it at the same place, once.
        let known = match &self {
            Self::UnknownImports { imports } => imports.contains(&import),
            first => *first == import,
        };
        if known {
            return self;
        }
        let mut imports = match self {
            Self::UnknownImports { imports } => imports.into_vec(),
            first => vec![first],
        };
        imports.push(import);
        imports.sort_by_key(Self::primary_span);
        Self::UnknownImports {
            imports: imports.into_boxed_slice(),
        }
    }

    /// rustc's E0432 message for an unresolved import: "unresolved import
    /// `a::b`", or "unresolved imports `a::b`, `a::c`" for several members
    /// of one `use`.
    pub(crate) fn unresolved_import_message(&self) -> Option<String> {
        match self {
            Self::UnknownImport { path, .. } => Some(format!("unresolved import `{path}`")),
            Self::UnknownImports { imports } => {
                let paths = imports
                    .iter()
                    .map(|import| format!("`{}`", import.name()))
                    .collect::<Vec<_>>();
                Some(format!("unresolved imports {}", paths.join(", ")))
            }
            _ => None,
        }
    }

    /// This error, a use of a moved value where the use borrows it, as a
    /// format argument does: rustc's "borrow of moved value".
    pub(crate) fn borrowing(mut self) -> Self {
        if let Self::MovedValue { borrowed, .. } = &mut self {
            *borrowed = true;
        }
        self
    }

    /// This error, a use of a moved value that the borrow at `borrow`
    /// takes whole, as `&v` does: rustc's "borrow of moved value",
    /// labelled over the borrow.
    pub(crate) fn borrowed_by(mut self, borrow: Span) -> Self {
        if let Self::MovedValue {
            borrowed,
            reference,
            previous_iteration: false,
            ..
        } = &mut self
        {
            *borrowed = true;
            *reference = borrow;
        }
        self
    }

    /// The import `declaration` of `name`, the name at `named`, bound in
    /// `namespace` where `previous` bound it before: rustc's E0252 when an
    /// import did, a duplicate item at the name when a declaration did.
    pub(crate) fn duplicate(
        name: ImmutableString,
        named: Span,
        namespace: &'static str,
        declaration: Import,
        previous: (Span, Option<Import>),
    ) -> Self {
        match previous {
            (_, Some(previous_declaration)) => Self::DuplicateImport {
                name,
                namespace,
                declaration: Box::new(declaration),
                previous_declaration: Box::new(previous_declaration),
            },
            (previous_declaration, None) => Self::DuplicateItem {
                name,
                declaration: named,
                previous_declaration,
            },
        }
    }

    /// rustc's E0616 for a read of the field `field` of the struct `owner`
    /// from outside the module the field is private to.
    pub(crate) fn private_field_read(owner: &str, field: &str, reference: Span) -> Self {
        Self::private_field("E0616", owner, field, reference)
    }

    /// rustc's E0451 for a struct literal or pattern that names the field
    /// `field` of the struct `owner` from outside the module the field is
    /// private to.
    pub(crate) fn private_field_named(owner: &str, field: &str, reference: Span) -> Self {
        Self::private_field("E0451", owner, field, reference)
    }

    fn private_field(code: &'static str, owner: &str, field: &str, reference: Span) -> Self {
        Self::StdPath {
            code,
            message: format!("field `{field}` of struct `{owner}` is private").into(),
            label: Some("private field".into()),
            note: None,
            reference,
        }
    }

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
            arguments: None,
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

    /// rustc's E0596 for `&mut place`, the borrow at `access`, of `place`,
    /// `binding` or a part of it, a local not declared `mut`, whose name is
    /// at `declared` where it is declared.
    pub(crate) fn immutable_borrow(
        place: &str,
        binding: &str,
        access: Span,
        declared: Span,
    ) -> Self {
        let message = if place == binding {
            format!("cannot borrow `{binding}` as mutable, as it is not declared as mutable")
        } else {
            format!("cannot borrow `{place}` as mutable, as `{binding}` is not declared as mutable")
        };
        Self::ImmutableBorrow {
            message: message.into(),
            access,
            declared,
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
            Self::DuplicateImport { .. } => Category::DuplicateImport,
            Self::UnknownType { .. } => Category::UnknownType,
            Self::UnknownImport { .. } | Self::UnknownImports { .. } => Category::UnknownImport,
            Self::InvalidImport { .. } => Category::InvalidImport,
            Self::InvalidTypeArguments { .. } => Category::InvalidTypeArguments,
            Self::LoweringUnavailable { .. } => Category::LoweringUnavailable,
            Self::InvalidDerive { .. } | Self::ExpansionRequired { .. } => Category::InvalidDerive,
            Self::DuplicateMember { .. } => Category::DuplicateMember,
            Self::UnknownMember { .. } | Self::NoItem { .. } | Self::MemberKind { .. } => {
                Category::UnknownMember
            }
            Self::NotInModule { .. } => Category::NotInModule,
            Self::UnknownFunction { .. } => Category::UnknownFunction,
            Self::UnknownVariable { .. } => Category::UnknownVariable,
            Self::UnknownMacro { .. } | Self::MacroExpansionRequired { .. } => {
                Category::UnknownMacro
            }
            Self::WrongKind { .. } => Category::WrongKind,
            Self::Unimplemented { .. } => Category::Unimplemented,
            Self::MovedValue { .. } => Category::MovedValue,
            Self::BorrowConflict { .. } | Self::ImmutableBorrow { .. } => Category::BorrowConflict,
            Self::InvalidConstruction { .. } => Category::InvalidConstruction,
            Self::TypeMismatch { .. } | Self::Unboxed { .. } => Category::TypeMismatch,
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
            Self::NonExhaustive { .. } => Category::NonExhaustive,
            Self::Refutable { .. } => Category::Refutable,
            Self::PatternFields { .. } => Category::PatternFields,
            Self::UnboundAlternative { .. } => Category::UnboundAlternative,
            Self::ArrayPatternLength { .. } => Category::InvalidMatch,
            Self::PatternMismatch { .. } => Category::TypeMismatch,
            Self::SlicePattern { .. } => Category::InvalidMatch,
            Self::RepeatedRest { .. } => Category::Syntax,
            Self::UnsupportedNominalSyntax { .. } => Category::UnsupportedNominalSyntax,
            Self::Syntax { .. } => Category::Syntax,
            Self::UnlabeledJumpInWhileCondition { .. } => Category::UnlabeledJumpInWhileCondition,
            Self::AsyncMain { .. } => Category::AsyncMain,
            Self::UnscopedWrite { .. } => Category::UnscopedWrite,
            Self::UnscopedMethod { .. } => Category::UnscopedMethod,
            Self::InvalidAssignee { .. } => Category::InvalidAssignee,
            Self::StdPath { .. } | Self::UnknownTrait { .. } => Category::StdPath,
            Self::NotYet { .. } => Category::NotYet,
            Self::TypeAnnotationsNeeded { .. } => Category::TypeAnnotationsNeeded,
            Self::ArgumentCount { .. } => Category::ArgumentCount,
            Self::CannotCompare { .. } => Category::CannotCompare,
            Self::BinaryOperation { .. } => Category::BinaryOperation,
            Self::ArgumentSignature { .. } => Category::ArgumentSignature,
            Self::PrivateItem { .. } => Category::PrivateItem,
            Self::OverflowingLiteral { .. } => Category::OverflowingLiteral,
            Self::KnownPanic { .. } => Category::KnownPanic,
            Self::Uninitialized { .. } => Category::Uninitialized,
            Self::LocalTypeNeeded { .. } => Category::LocalTypeNeeded,
        }
    }

    /// Return the declaration or type name involved in the failure.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::DuplicateItem { name, .. }
            | Self::DuplicateImport { name, .. }
            | Self::UnknownType { name, .. }
            | Self::UnknownFunction { name, .. }
            | Self::UnknownVariable { name, .. }
            | Self::UnknownMacro { name, .. }
            | Self::MacroExpansionRequired { path: name, .. }
            | Self::WrongKind { name, .. }
            | Self::MovedValue { name, .. } => name.as_str(),
            Self::BorrowConflict { message, .. } | Self::ImmutableBorrow { message, .. } => {
                message.as_str()
            }
            Self::UnknownImport { path, .. } | Self::InvalidImport { path, .. } => path.as_str(),
            Self::UnknownImports { imports } => imports[0].name(),
            Self::InvalidTypeArguments { ty, .. } => ty.as_str(),
            Self::LoweringUnavailable { item, .. } => item.as_str(),
            Self::InvalidDerive { owner, .. }
            | Self::ExpansionRequired { owner, .. }
            | Self::DuplicateMember { owner, .. } => owner.as_str(),
            Self::NotInModule { module, .. } => module.as_str(),
            Self::PrivateItem { name, .. } => name.as_str(),
            Self::OverflowingLiteral { ty, .. } => ty.as_str(),
            Self::KnownPanic { label, .. } => label.as_str(),
            Self::Uninitialized { name, .. }
            | Self::LocalTypeNeeded { name, .. }
            | Self::UnboundAlternative { name, .. } => name.as_str(),
            Self::UnknownMember { owner, .. }
            | Self::NoItem { owner, .. }
            | Self::MemberKind { owner, .. }
            | Self::InvalidConstruction { owner, .. }
            | Self::MissingCapability { owner, .. }
            | Self::InvalidMatch { owner, .. }
            | Self::PatternFields { owner, .. }
            | Self::ArrayPatternLength { owner, .. } => owner.as_str(),
            Self::PatternMismatch { types, .. } => types.expected.as_str(),
            Self::SlicePattern { ty, .. } => ty.as_str(),
            Self::RepeatedRest { .. } => "..",
            Self::NonExhaustive { ty, .. } | Self::Refutable { ty, .. } => ty.as_str(),
            Self::TypeMismatch { expected, .. }
            | Self::Unboxed { expected, .. }
            | Self::IfWithoutElse { expected, .. } => expected.as_str(),
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
            Self::UnscopedWrite { owner, .. } => owner.as_str(),
            Self::UnscopedMethod { member, .. } => member.as_str(),
            Self::InvalidAssignee { .. } => "assignment",
            Self::StdPath { message, .. } => message.as_str(),
            Self::UnknownTrait { name, .. } => name.as_str(),
            Self::NotYet { path, .. } => path.as_str(),
            Self::TypeAnnotationsNeeded { parameter, .. } => parameter.as_str(),
            Self::ArgumentCount { name, .. } => name.as_str(),
            Self::CannotCompare { lhs, .. } => lhs.as_str(),
            Self::BinaryOperation { operation, .. } => operation.message.as_str(),
            Self::ArgumentSignature { expected, .. } => expected.as_str(),
        }
    }

    /// Return the span callers should present as the primary diagnostic label.
    #[must_use]
    pub const fn primary_span(&self) -> Span {
        match self {
            Self::DuplicateItem { declaration, .. } => *declaration,
            Self::DuplicateImport { declaration, .. } => declaration.written,
            Self::UnknownType { reference, .. } => *reference,
            Self::UnknownImport { reference, .. }
            | Self::InvalidImport { reference, .. }
            | Self::InvalidTypeArguments { reference, .. } => *reference,
            Self::UnknownImports { imports } => imports[0].primary_span(),
            Self::LoweringUnavailable { body, .. } => *body,
            Self::InvalidDerive { attribute, .. } | Self::ExpansionRequired { attribute, .. } => {
                *attribute
            }
            Self::DuplicateMember { declaration, .. } => *declaration,
            Self::UnknownMember { reference, .. }
            | Self::NoItem { reference, .. }
            | Self::MemberKind { reference, .. }
            | Self::NotInModule { reference, .. }
            | Self::PrivateItem { reference, .. } => *reference,
            Self::OverflowingLiteral { literal, .. } => *literal,
            Self::KnownPanic { at, .. } => *at,
            Self::Uninitialized { reference, .. } => *reference,
            Self::LocalTypeNeeded { declaration, .. } => *declaration,
            Self::UnknownFunction { reference, .. } => *reference,
            Self::UnknownVariable { reference, .. }
            | Self::UnknownMacro { reference, .. }
            | Self::MacroExpansionRequired { reference, .. }
            | Self::WrongKind { reference, .. } => *reference,
            Self::MovedValue { reference, .. } => *reference,
            Self::BorrowConflict { access, .. } | Self::ImmutableBorrow { access, .. } => *access,
            Self::InvalidConstruction { construction, .. } => *construction,
            Self::TypeMismatch { expression, .. }
            | Self::Unboxed { expression, .. }
            | Self::IfWithoutElse { expression, .. } => *expression,
            Self::ReturnWithoutValue { at, .. } => *at,
            Self::ImmutableBinding { mutation, .. } => *mutation,
            Self::InvalidImplOwner { reference, .. } => *reference,
            Self::DuplicateMethod { declaration, .. } => *declaration,
            Self::InvalidMethodCall { call, .. } => *call,
            Self::InvalidHostOperation { operation, .. }
            | Self::ImportedApplicationRequired { operation, .. } => *operation,
            Self::UnusedValue { expression, .. } => *expression,
            Self::SelfOutsideImpl { reference, .. } => *reference,
            Self::ImmutableReceiver { mutation, .. } => *mutation,
            Self::InvalidMethodSignature { signature, .. } => *signature,
            Self::MissingCapability { operation, .. } => *operation,
            Self::InvalidMatch { pattern, .. } => *pattern,
            Self::NonExhaustive { scrutinee, .. } => *scrutinee,
            Self::Refutable { pattern, .. }
            | Self::PatternFields { pattern, .. }
            | Self::UnboundAlternative { pattern, .. }
            | Self::ArrayPatternLength { pattern, .. }
            | Self::PatternMismatch { pattern, .. }
            | Self::SlicePattern { pattern, .. } => *pattern,
            Self::RepeatedRest { rest, .. } => *rest,
            Self::UnsupportedNominalSyntax { source, .. } => *source,
            Self::AsyncMain { at }
            | Self::UnscopedWrite { at, .. }
            | Self::UnscopedMethod { at, .. } => *at,
            Self::InvalidAssignee { operator, .. } => *operator,
            Self::StdPath { reference, .. }
            | Self::UnknownTrait { reference, .. }
            | Self::NotYet { reference, .. } => *reference,
            Self::TypeAnnotationsNeeded { reference, .. } => *reference,
            Self::ArgumentCount { call, .. } => *call,
            Self::Syntax { at, .. }
            | Self::Unimplemented { at, .. }
            | Self::UnlabeledJumpInWhileCondition { at, .. }
            | Self::CannotCompare { at, .. }
            | Self::BinaryOperation { at, .. }
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
            Self::DuplicateImport {
                previous_declaration,
                ..
            } => Some(previous_declaration.written),
            Self::InvalidDerive { declaration, .. } => Some(*declaration),
            Self::ExpansionRequired { .. } => None,
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
            Self::InvalidAssignee { assignee, .. } => Some(*assignee),
            Self::UnboundAlternative { binding, .. } => Some(*binding),
            Self::RepeatedRest { previous, .. } => Some(*previous),
            Self::PatternMismatch {
                origin: Some(origin),
                ..
            } => Some(origin.span()),
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
            | Self::UnknownImports { .. }
            | Self::InvalidTypeArguments { .. }
            | Self::LoweringUnavailable { .. } => None,
            Self::UnknownMember { .. }
            | Self::NoItem { .. }
            | Self::MemberKind { .. }
            | Self::NotInModule { .. }
            | Self::UnknownFunction { .. }
            | Self::UnknownVariable { .. }
            | Self::UnknownMacro { .. }
            | Self::MacroExpansionRequired { .. }
            | Self::WrongKind { .. }
            | Self::Unimplemented { .. }
            | Self::InvalidConstruction { .. }
            | Self::TypeMismatch { .. }
            | Self::Unboxed { .. }
            | Self::IfWithoutElse { .. }
            | Self::ReturnWithoutValue { .. }
            | Self::MissingCapability { .. }
            | Self::InvalidMatch { .. }
            | Self::NonExhaustive { .. }
            | Self::Refutable { .. }
            | Self::PatternFields { .. }
            | Self::ArrayPatternLength { .. }
            | Self::PatternMismatch { origin: None, .. }
            | Self::SlicePattern { .. }
            | Self::UnsupportedNominalSyntax { .. }
            | Self::Syntax { .. }
            | Self::AsyncMain { .. }
            | Self::UnscopedWrite { .. }
            | Self::UnscopedMethod { .. }
            | Self::ImmutableBorrow { .. }
            | Self::StdPath { .. }
            | Self::UnknownTrait { .. }
            | Self::NotYet { .. }
            | Self::TypeAnnotationsNeeded { .. }
            | Self::CannotCompare { .. }
            | Self::BinaryOperation { .. }
            | Self::ArgumentSignature { .. }
            | Self::OverflowingLiteral { .. }
            | Self::KnownPanic { .. } => None,
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

/// rustc's message for `self` where no method's receiver is: a parameter of
/// a function outside an `impl` or a `trait` (`true`), which rustc reports
/// without a code, or a value read where no `self` parameter is (E0424).
pub(crate) struct SelfOutside(pub(crate) bool);

impl SelfOutside {
    /// rustc's label on `self`.
    pub(crate) fn label(&self) -> &'static str {
        if self.0 {
            "not semantically valid as function parameter"
        } else {
            "`self` value is a keyword only available in methods with a `self` parameter"
        }
    }
}

impl fmt::Display for SelfOutside {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(if self.0 {
            "`self` parameter is only allowed in associated functions"
        } else {
            "expected value, found module `self`"
        })
    }
}

/// The type an E0599 is about, as rustc names it: the program's own struct
/// or enum, or a reference.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Receiver {
    /// What rustc calls it: `struct`, `enum`, `reference` or `mutable
    /// reference`.
    pub kind: &'static str,
    /// The type as the message names it: a struct or enum with its
    /// declared parameters, `G<T>`.
    pub declared: ImmutableString,
    /// The type as the label names it, the one found: `G<i32>`.
    pub found: ImmutableString,
    /// The struct's or enum's name where the program declares it, in the
    /// error's file: rustc marks its definition.
    pub definition: Option<Span>,
}

/// An item of the type an E0599 is about whose name is like the missing
/// one's, which rustc suggests: "there is a method `go` with a similar
/// name".
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Similar {
    /// The item's name.
    pub name: ImmutableString,
    /// What rustc calls it: `method`, `associated function` or
    /// `associated constant`.
    pub kind: &'static str,
    /// The item's name where the program declares it.
    pub declared: Span,
    /// How rustc shows it.
    pub shown: Shown,
}

/// How rustc shows a [`Similar`] item.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Shown {
    /// In place of the missing name, which the call's arguments fit.
    Replaced,
    /// At its definition.
    Defined,
    /// At its definition, a method whose parameters the call's arguments
    /// don't fit: "but with different arguments".
    DifferentArguments,
}

/// rustc's E0599 message for the method `member` of a trait out of scope,
/// on a `kind` (`type`, `struct`, ..) `owner`.
pub(crate) fn unscoped_method(kind: &str, owner: &str, member: &str) -> String {
    format!("no method named `{member}` found for {kind} `{owner}` in the current scope")
}

/// rustc's message for a `match` that misses the values of `witnesses`
/// (E0004): `non-exhaustive patterns: `A` and `B` not covered`.
pub(crate) struct NonExhaustive<'a>(pub(crate) &'a [ImmutableString]);

impl NonExhaustive<'_> {
    /// The witnesses rustc names: three at most, and how many more.
    const SHOWN: usize = 3;

    /// rustc's label on the scrutinee: `pattern `A` not covered`.
    pub(crate) fn label(&self) -> String {
        let noun = if self.0.len() == 1 {
            "pattern"
        } else {
            "patterns"
        };
        format!("{noun} {} not covered", self.uncovered())
    }

    /// rustc's help, and the pattern of the arm it suggests: the one
    /// witness, the witnesses as one or-pattern, or `_` past three.
    pub(crate) fn help(&self) -> (String, String) {
        let (how, pattern) = match self.0 {
            [one] => (" or an explicit pattern as shown", one.to_string()),
            witnesses if witnesses.len() <= Self::SHOWN => (
                ", a match arm with multiple or-patterns as shown, or multiple match arms",
                witnesses.join(" | "),
            ),
            _ => (" as shown, or multiple match arms", "_".to_string()),
        };
        (
            format!(
                "ensure that all possible cases are being handled by adding a match arm \
                 with a wildcard pattern{how}"
            ),
            pattern,
        )
    }

    /// The witnesses as rustc joins them: `` `A`, `B` and `C` ``, or
    /// `` `A`, `B`, `C` and 2 more ``.
    fn uncovered(&self) -> String {
        let quoted = |witnesses: &[ImmutableString]| witnesses.join("`, `");
        match self.0 {
            [one] => format!("`{one}`"),
            [head @ .., last] if self.0.len() <= Self::SHOWN => {
                format!("`{}` and `{last}`", quoted(head))
            }
            witnesses => format!(
                "`{}` and {} more",
                quoted(&witnesses[..Self::SHOWN]),
                witnesses.len() - Self::SHOWN
            ),
        }
    }
}

impl fmt::Display for NonExhaustive<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "non-exhaustive patterns: {} not covered",
            self.uncovered()
        )
    }
}

/// Where an unresolved `use` stops resolving, as rustc's E0432 labels it.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Unresolved {
    /// The resolver knows only that the path names nothing: no label.
    Path,
    /// `use name;` names no crate: "no external crate `name`".
    Crate,
    /// A glob's first segment names no crate or module in scope: "use of
    /// unresolved module or unlinked crate `name`".
    Unlinked,
    /// The module before the last segment, as rustc names it, has no item
    /// of that name: "no `Missing` in `widget`", at the last segment
    /// while the error marks the whole import.
    Item {
        /// The module, as rustc names it.
        module: ImmutableString,
        /// The last segment, which the label is at.
        segment: Span,
    },
    /// The module before the last segment, as rustc names it, has no
    /// module of that name: "could not find `nothing` in `lib`".
    Module(ImmutableString),
    /// The segment names a value of the module before it, such as a
    /// function or a constant, where the path needs a module or a type:
    /// "expected type, found constant `Center` in `iced`".
    Value {
        /// The value's kind as rustc names it: `function` or `constant`.
        noun: &'static str,
        /// The module, as rustc names it.
        module: ImmutableString,
    },
}

impl Unresolved {
    /// rustc's label at the unresolved `path`, when it has one.
    pub(crate) fn label(&self, path: &str) -> Option<String> {
        let last = path.rsplit("::").next().unwrap_or(path);
        match self {
            Self::Path => None,
            Self::Crate => Some(format!("no external crate `{path}`")),
            Self::Unlinked => Some(NotFound::Module(path).undeclared()),
            Self::Item { module, .. } => Some(format!("no `{last}` in `{module}`")),
            // rustc names the module the segment is missing from by its own
            // name.
            Self::Module(module) => Some(format!(
                "could not find `{last}` in `{}`",
                module.rsplit("::").next().unwrap_or(module)
            )),
            Self::Value { noun, module } => Some(format!(
                "expected type, found {noun} `{last}` in `{module}`"
            )),
        }
    }
}

/// Where a pattern that must match every value is written, as rustc names
/// it in E0005.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum BindingSite {
    /// `let pattern = value;`: where its `let` and its value are, and
    /// whether the pattern binds a name.
    Local {
        /// The `let` keyword's start.
        keyword: crate::Position,
        /// The value's end, before the `;`.
        value: crate::Position,
        /// Whether the pattern binds a name, which `let else` keeps.
        binds: bool,
    },
    /// `for pattern in values`.
    ForLoop,
    /// A function's parameter.
    Argument,
    /// A destructuring assignment's assignee, `Some(x) = value`.
    Assignment,
}

/// rustc's E0005 for a pattern at `site` that misses the values of
/// `witnesses`: `refutable pattern in local binding`.
pub(crate) struct Refutable<'a> {
    pub(crate) witnesses: &'a [ImmutableString],
    pub(crate) site: &'a BindingSite,
}

impl Refutable<'_> {
    /// rustc's label on the pattern: `pattern `None` not covered`.
    pub(crate) fn label(&self) -> String {
        NonExhaustive(self.witnesses).label()
    }

    /// rustc's notes on what a `let` requires, before the matched type's.
    pub(crate) fn notes(&self) -> &'static [&'static str] {
        match self.site {
            BindingSite::Local { .. } => &[
                "`let` bindings require an \"irrefutable pattern\", like a `struct` or an \
                 `enum` with only one variant",
                "for more information, visit \
                 https://doc.rust-lang.org/book/ch19-02-refutability.html",
            ],
            BindingSite::Assignment => &[
                "destructuring assignments require an \"irrefutable pattern\", like a `struct` \
                 or an `enum` with only one variant",
                "for more information, visit \
                 https://doc.rust-lang.org/book/ch19-02-refutability.html",
            ],
            BindingSite::ForLoop | BindingSite::Argument => &[],
        }
    }

    /// rustc's suggestion for a `let`, its message and the code it puts
    /// at each place: `else { todo!() }` after the value when the pattern
    /// binds a name, which `let...else` keeps, else `if` before the `let`
    /// and a block after the value, which ignores the values it misses.
    pub(crate) fn suggestion(&self) -> Option<(String, Vec<(crate::Position, &'static str)>)> {
        let BindingSite::Local {
            keyword,
            value,
            binds,
        } = self.site
        else {
            return None;
        };
        let variants = if self.witnesses.len() == 1 {
            "variant that isn't"
        } else {
            "variants that aren't"
        };
        Some(if *binds {
            (
                format!("you might want to use `let...else` to handle the {variants} matched"),
                vec![(*value, " else { todo!() }")],
            )
        } else {
            (
                format!("you might want to use `if let` to ignore the {variants} matched"),
                vec![(*keyword, "if "), (*value, " { todo!() }")],
            )
        })
    }
}

impl fmt::Display for Refutable<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let origin = match self.site {
            BindingSite::Local { .. } => "local binding",
            BindingSite::ForLoop => "`for` loop binding",
            BindingSite::Argument => "function argument",
            BindingSite::Assignment => "assignment",
        };
        write!(formatter, "refutable pattern in {origin}")
    }
}

/// rustc's E0023 for a tuple struct or tuple variant pattern of `found`
/// fields where its type declares `expected`: "this pattern has 1 field,
/// but the corresponding tuple variant has 2 fields".
pub(crate) struct PatternFields {
    pub(crate) variant: bool,
    pub(crate) expected: usize,
    pub(crate) found: usize,
}

impl PatternFields {
    const fn noun(&self) -> &'static str {
        if self.variant {
            "tuple variant"
        } else {
            "tuple struct"
        }
    }

    fn fields(count: usize) -> String {
        format!("{count} field{}", if count == 1 { "" } else { "s" })
    }

    /// rustc's label on the field patterns: "expected 2 fields, found 1".
    pub(crate) fn label(&self) -> String {
        format!(
            "expected {}, found {}",
            Self::fields(self.expected),
            self.found
        )
    }

    /// rustc's label on the declared fields: "tuple variant has 2 fields".
    pub(crate) fn declared(&self) -> String {
        format!("{} has {}", self.noun(), Self::fields(self.expected))
    }
}

impl fmt::Display for PatternFields {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "this pattern has {}, but the corresponding {} has {}",
            Self::fields(self.found),
            self.noun(),
            Self::fields(self.expected)
        )
    }
}

/// `count` elements, as rustc counts them: "1 element", "2 elements".
fn elements(count: usize) -> String {
    format!("{count} element{}", if count == 1 { "" } else { "s" })
}

/// rustc's E0527 for an array pattern of `fixed` elements on an array of
/// `length`, or its E0528 for one with a `..` (`rest`) of more.
pub(crate) struct ArrayLength {
    pub(crate) fixed: usize,
    pub(crate) length: usize,
    pub(crate) rest: bool,
}

impl ArrayLength {
    /// rustc's code: E0528 with a `..`, else E0527.
    pub(crate) const fn code(&self) -> &'static str {
        if self.rest {
            "E0528"
        } else {
            "E0527"
        }
    }

    /// rustc's label on the pattern: "expected 3 elements".
    pub(crate) fn label(&self) -> String {
        if self.rest {
            format!("pattern cannot match array of {}", elements(self.length))
        } else {
            format!("expected {}", elements(self.length))
        }
    }
}

impl fmt::Display for ArrayLength {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "pattern requires {}{} but array has {}",
            if self.rest { "at least " } else { "" },
            elements(self.fixed),
            self.length
        )
    }
}

/// rustc's E0369 for a binary operator whose left operand's type has no
/// impl of the operator's trait, or its E0368 for a compound assignment:
/// its message, and what its labels and note name.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct BinaryOperation {
    /// rustc's message: "binary operation `==` cannot be applied to type
    /// `Value`".
    pub(crate) message: ImmutableString,
    /// Each span rustc labels and its label: each operand with its type,
    /// or an assignment's left operand with "cannot use `+=` on type
    /// `Value`".
    pub(crate) labels: Box<[(Span, ImmutableString)]>,
    /// The program's own type the left operand is, where the program
    /// declares it in the error's file.
    pub(crate) missing: Option<MissingImpl>,
    /// Whether the operator assigns its result to its left operand, which
    /// rustc refuses as E0368.
    pub(crate) assigns: bool,
}

impl BinaryOperation {
    /// rustc's code for the operation.
    pub(crate) fn code(&self) -> &'static str {
        if self.assigns {
            "E0368"
        } else {
            "E0369"
        }
    }
}

/// A trait the program's own type has no impl of, where rustc notes the
/// impl might be missing and suggests deriving it.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct MissingImpl {
    /// The trait.
    pub(crate) capability: ImmutableString,
    /// The right operand's type, where it differs from the left's: rustc
    /// names the trait with it, as `Add<{integer}>`.
    pub(crate) operand: Option<ImmutableString>,
    /// The type, as the program names it.
    pub(crate) owner: ImmutableString,
    /// The type's name in its declaration.
    pub(crate) declared: Span,
}

impl MissingImpl {
    /// The trait as rustc's note writes it: `Add`, `Shl<{integer}>`.
    pub(crate) fn written(&self) -> String {
        match &self.operand {
            Some(operand) => format!("{}<{operand}>", self.capability),
            None => self.capability.to_string(),
        }
    }
}

/// The type a pattern matches and the pattern's own, as rustc's E0308
/// on the pattern names them.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PatternTypes {
    /// The matched type's name.
    pub expected: ImmutableString,
    /// The pattern's type, a `_` for each part it leaves open: `(_, _)`,
    /// `Option<_>`.
    pub found: ImmutableString,
    /// rustc's sorts of the two types in its "expected .. found" note
    /// (`type`, `tuple`, `enum`), or `None` where both types are simple
    /// and rustc writes no note.
    pub sorts: Option<(&'static str, &'static str)>,
    /// The tuple's arity and the tuple pattern's, which rustc's label
    /// names instead of the types.
    pub arity: Option<(usize, usize)>,
    /// The binding a `let`'s or an arm's whole pattern, a `&mut` of it,
    /// names on a `&` value: rustc suggests declaring it `mut` instead.
    pub binding: Option<MutBinding>,
}

/// One binding under a `&mut` pattern on a `&` value, the whole pattern of
/// what holds it: rustc suggests the binding `mut` instead.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct MutBinding {
    /// The binding's name.
    pub name: ImmutableString,
    /// The binding, after the `&mut`.
    pub span: Span,
    /// Whether an arm holds the pattern, where rustc notes the `mut`
    /// binding and suggests removing the `&mut`, rather than a `let`,
    /// where it suggests the `mut` binding in place.
    pub arm: bool,
}

impl PatternTypes {
    /// rustc's label on the pattern: "expected integer, found `(_, _)`",
    /// "expected a tuple with 3 elements, found one with 2 elements", or
    /// for references of the other mutability "types differ in
    /// mutability".
    pub(crate) fn label(&self) -> String {
        if let Some(("reference", "mutable reference") | ("mutable reference", "reference")) =
            self.sorts
        {
            return "types differ in mutability".to_string();
        }
        match self.arity {
            Some((arity, found)) => format!(
                "expected a tuple with {}, found one with {}",
                elements(arity),
                elements(found)
            ),
            None => format!(
                "expected {}, found {}",
                Described(&self.expected),
                Described(&self.found)
            ),
        }
    }

    /// rustc's note: "expected type `{integer}`" over "found tuple
    /// `(_, _)`", the shorter lead padded so the types line up.
    pub(crate) fn note(&self) -> Option<String> {
        let (expected, found) = self.sorts?;
        let expected = format!("expected {expected}");
        let found = format!("found {found}");
        let width = expected.len().max(found.len());
        Some(format!(
            "{expected:>width$} `{}`\n{found:>width$} `{}`",
            self.expected, self.found
        ))
    }
}

/// What gave the value a pattern matches its type, which rustc labels
/// beside a pattern of another type.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum PatternOrigin {
    /// The matched expression, of type `ty`.
    Expression {
        /// The expression.
        span: Span,
        /// Its type's name.
        ty: ImmutableString,
    },
    /// A `let`'s or a parameter's written type.
    Annotation(Span),
    /// A `for` loop's iterator, of items of type `item`.
    Iterator {
        /// The iterator expression.
        span: Span,
        /// Its items' type name.
        item: ImmutableString,
    },
}

impl PatternOrigin {
    /// Where rustc's label goes.
    #[must_use]
    pub const fn span(&self) -> Span {
        match self {
            Self::Expression { span, .. } | Self::Iterator { span, .. } => *span,
            Self::Annotation(span) => *span,
        }
    }

    /// rustc's label: "this expression has type `T`".
    pub(crate) fn label(&self) -> String {
        match self {
            Self::Expression { ty, .. } => format!("this expression has type `{ty}`"),
            Self::Annotation(_) => "expected due to this".to_string(),
            Self::Iterator { item, .. } => {
                format!("this is an iterator with items of type `{item}`")
            }
        }
    }
}

impl Error {
    /// This error, with `origin` as what gave the matched value its type
    /// when it is a pattern's mismatch that names none yet: the innermost
    /// statement or arm a pattern heads names it. A slice pattern on a
    /// `Vec` takes the matched expression as where rustc's slicing goes.
    #[must_use]
    pub(crate) fn matched(mut self, origin: impl FnOnce() -> PatternOrigin) -> Self {
        match &mut self {
            Self::PatternMismatch {
                origin: unnamed @ None,
                ..
            } => *unnamed = Some(origin()),
            // A slice pattern is its statement's whole pattern when the
            // matched expression is of the pattern's type, or a reference
            // to it.
            Self::SlicePattern {
                ty,
                sliceable: true,
                subject: subject @ None,
                ..
            } => {
                if let PatternOrigin::Expression { span, ty: matched } = origin()
                    && matched
                        .trim_start_matches(['&', ' '])
                        .trim_start_matches("mut ")
                        == ty.as_str()
                {
                    *subject = Some(span);
                }
            }
            _ => {}
        }
        self
    }
}

/// rustc's E0529 for a slice pattern on a value of type `0`.
pub(crate) struct SlicePattern<'a>(pub(crate) &'a str);

impl SlicePattern<'_> {
    /// rustc's label on the pattern.
    pub(crate) fn label(&self) -> String {
        format!("pattern cannot match with input type `{}`", self.0)
    }
}

impl fmt::Display for SlicePattern<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "expected an array or slice, found `{}`", self.0)
    }
}

/// rustc's error for a second `..` in one tuple pattern, or in one slice
/// pattern (`true`).
pub(crate) struct RepeatedRest(pub(crate) bool);

impl RepeatedRest {
    /// rustc's label on the second `..`.
    pub(crate) fn label(&self) -> String {
        let pattern = if self.0 { "slice" } else { "tuple" };
        format!("can only be used once per {pattern} pattern")
    }
}

impl fmt::Display for RepeatedRest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "`..` {}", self.label())
    }
}

/// rustc's E0408 for `0`, a name some alternatives of an or-pattern bind
/// and another does not: "variable `x` is not bound in all patterns".
pub(crate) struct UnboundAlternative<'a>(pub(crate) &'a str);

impl UnboundAlternative<'_> {
    /// rustc's label on the alternative that does not bind it.
    pub(crate) fn label(&self) -> String {
        format!("pattern doesn't bind `{}`", self.0)
    }
}

impl fmt::Display for UnboundAlternative<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "variable `{}` is not bound in all patterns",
            self.0
        )
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

/// rustc's message for an assignment to a value that is no place (E0070,
/// E0067).
pub const INVALID_ASSIGNEE: &str = "invalid left-hand side of assignment";

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
            // rustc reads a call of a capitalized name as a tuple struct's
            // or variant's too.
            Self::Function(name)
                if name
                    .rsplit("::")
                    .next()
                    .is_some_and(|last| last.starts_with(char::is_uppercase)) =>
            {
                ("function, tuple struct or tuple variant", *name)
            }
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

/// How rustc's help says to make the `Box<dyn Error>` a value is not.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Boxing {
    /// `Box::new(value)`, for a value whose type implements `Error`.
    New,
    /// `value.into()`, for text of this type: `&'static str` or `String`.
    Into(ImmutableString),
}

impl Boxing {
    /// rustc's note beside its `Box::new` help.
    pub(crate) const HEAP_NOTE: &'static str = "for more on the distinction between the stack \
        and the heap, read https://doc.rust-lang.org/book/ch15-01-box.html, \
        https://doc.rust-lang.org/rust-by-example/std/box.html, and \
        https://doc.rust-lang.org/std/boxed/index.html";
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
            // A function's `-> impl Shape` (D1109).
            ty if ty.starts_with("impl ") => formatter.write_str("opaque type"),
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

/// A raw identifier reads as Rust writes it in every message
/// (`tokenizer::raw_names`), and an
/// item reached through a crate's hidden segment reads from that crate's
/// root (`host::path::public`).
impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = Written(self).to_string();
        let text = crate::tokenizer::raw_names(&text);
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
            Error::DuplicateImport {
                name,
                declaration,
                previous_declaration,
                ..
            } => write!(
                formatter,
                "the name `{name}` is defined multiple times at {}; previously imported at {}",
                declaration.written, previous_declaration.written
            ),
            Error::UnknownType {
                name,
                reference,
                path,
                ..
            } => write!(
                formatter,
                "{} at {reference} ({})",
                NotFound::type_named(name, *path),
                if *path { "E0433" } else { "E0425" }
            ),
            Error::UnknownImport {
                path, reference, ..
            } => {
                write!(formatter, "unresolved import `{path}` at {reference} (E0432)")
            }
            Error::UnknownImports { imports } => {
                let shown = imports.iter().map(ToString::to_string).collect::<Vec<_>>();
                formatter.write_str(&shown.join("; "))
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
            Error::ExpansionRequired {
                owner, attribute, ..
            } => write!(
                formatter,
                "the derives on `{owner}` at {attribute} have not been expanded"
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
            Error::NoItem {
                member,
                reference,
                receiver,
                ..
            } => write!(
                formatter,
                "no item `{member}` of {} `{}` at {reference} (E0599)",
                receiver.kind, receiver.declared
            ),
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
                ..
            } => write!(formatter, "field `{owner}::{member}` called like a method at {reference}"),
            Error::MemberKind {
                owner,
                member,
                field: false,
                reference,
                ..
            } => write!(formatter, "method `{owner}::{member}` read like a field at {reference}"),
            Error::UnknownFunction {
                name, reference, ..
            } => write!(formatter, "{} at {reference} (E0425)", NotFound::Function(name)),
            Error::UnknownVariable {
                name, reference, ..
            } => {
                write!(formatter, "{} at {reference} (E0425)", NotFound::Value(name))
            }
            Error::UnknownMacro {
                name, reference, ..
            } => {
                write!(formatter, "{} at {reference}", NotFound::Macro(name))
            }
            Error::MacroExpansionRequired { path, reference, .. } => write!(
                formatter,
                "the call of `{path}!` at {reference} has not been expanded"
            ),
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
                partial,
                ..
            } => write!(
                formatter,
                "{} of {}moved value `{name}` at {reference}; it was already moved at {moved}",
                if *borrowed { "borrow" } else { "use" },
                if *partial { "partially " } else { "" }
            ),
            Error::BorrowConflict {
                code,
                message,
                access,
                ..
            } => write!(formatter, "{message} at {access} ({code})"),
            Error::ImmutableBorrow {
                message, access, ..
            } => write!(formatter, "{message} at {access} (E0596)"),
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
            }
            | Error::Unboxed {
                expected,
                actual,
                expression,
                ..
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
            Error::ReturnWithoutValue { output, at, .. } => write!(
                formatter,
                "{RETURN_WITHOUT_VALUE} at {at}: expected `{output}` (E0069)"
            ),
            Error::ImmutableBinding {
                binding,
                mutation,
                declaration,
                ..
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
            Error::SelfOutsideImpl {
                reference,
                parameter,
            } => write!(formatter, "{} at {reference}", SelfOutside(*parameter)),
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
            Error::NonExhaustive {
                witnesses,
                scrutinee,
                ..
            } => write!(formatter, "{} at {scrutinee} (E0004)", NonExhaustive(witnesses)),
            Error::Refutable {
                witnesses,
                site,
                pattern,
                ..
            } => write!(
                formatter,
                "{}: {} at {pattern} (E0005)",
                Refutable { witnesses, site },
                NonExhaustive(witnesses).label()
            ),
            Error::PatternFields {
                variant,
                expected,
                found,
                pattern,
                ..
            } => write!(
                formatter,
                "{} at {pattern} (E0023)",
                PatternFields {
                    variant: *variant,
                    expected: *expected,
                    found: *found,
                }
            ),
            Error::UnboundAlternative { name, pattern, .. } => {
                write!(formatter, "{} at {pattern} (E0408)", UnboundAlternative(name))
            }
            Error::ArrayPatternLength {
                fixed,
                length,
                rest,
                pattern,
                ..
            } => {
                let array = ArrayLength {
                    fixed: *fixed,
                    length: *length,
                    rest: *rest,
                };
                write!(formatter, "{array} at {pattern} ({})", array.code())
            }
            Error::PatternMismatch { types, pattern, .. } => write!(
                formatter,
                "mismatched types: {} at {pattern} (E0308)",
                types.label()
            ),
            Error::SlicePattern { ty, pattern, .. } => {
                write!(formatter, "{} at {pattern} (E0529)", SlicePattern(ty))
            }
            Error::RepeatedRest { slice, rest, .. } => {
                write!(formatter, "{} at {rest}", RepeatedRest(*slice))
            }
            Error::UnsupportedNominalSyntax { construct, source } => {
                write!(formatter, "{} at {source}", Unsupported(construct))
            }
            Error::Syntax { message, .. } => formatter.write_str(message),
            Error::UnlabeledJumpInWhileCondition { .. } => formatter
                .write_str("`break` or `continue` with no label in the condition of a `while` loop"),
            Error::AsyncMain { .. } => formatter.write_str(ASYNC_MAIN),
            Error::UnscopedWrite { owner, at, .. } => {
                write!(formatter, "cannot write into `{owner}` at {at}")
            }
            Error::UnscopedMethod {
                kind,
                owner,
                member,
                at,
                ..
            } => write!(formatter, "{} at {at}", unscoped_method(kind, owner, member)),
            Error::InvalidAssignee { operator, .. } => {
                write!(formatter, "{INVALID_ASSIGNEE} at {operator}")
            }
            Error::StdPath {
                code,
                message,
                reference,
                ..
            } => write!(formatter, "{message} at {reference} ({code})"),
            Error::UnknownTrait {
                name, reference, ..
            } => write!(
                formatter,
                "cannot find trait `{name}` in this scope at {reference} (E0405)"
            ),
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
            Error::KnownPanic { lint, label, at } => {
                write!(formatter, "{} at {at}: {label}", lint.message())
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
            Error::BinaryOperation { operation, at } => {
                write!(
                    formatter,
                    "{} at {at} ({})",
                    operation.message,
                    operation.code()
                )
            }
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
            "PartialEq" => write!(formatter, "can't compare `{0}` with `{0}`", self.0),
            "Send" => write!(
                formatter,
                "`{}` cannot be sent between threads safely",
                self.0
            ),
            "Sync" => write!(
                formatter,
                "`{}` cannot be shared between threads safely",
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
