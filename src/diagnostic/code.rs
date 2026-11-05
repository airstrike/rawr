//! Stable semantic diagnostic codes.

use super::Category;
use std::fmt;

/// One stable semantic diagnostic identifier.
///
/// Codes are intentionally closed: renderers and consumers may inspect them,
/// but only Rawr declares new values.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Code {
    value: &'static str,
    category: Category,
}

impl Code {
    const fn new(value: &'static str, category: Category) -> Self {
        Self { value, category }
    }

    /// Return the stable printable code: rustc's, as `E0308`, or empty for
    /// an error rustc reports without one.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.value
    }

    /// The code as rustc prints it after the severity: `[E0308]`, or
    /// nothing for an error rustc reports without one.
    #[must_use]
    pub const fn tag(self) -> Tag {
        Tag(self)
    }

    /// Return the semantic family fixed by this code.
    #[must_use]
    pub const fn category(self) -> Category {
        self.category
    }
}

impl fmt::Display for Code {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.value)
    }
}

/// A code as rustc prints it after the severity (`Code::tag`).
#[derive(Clone, Copy, Debug)]
pub struct Tag(Code);

impl fmt::Display for Tag {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0.value {
            "" => Ok(()),
            value => write!(formatter, "[{value}]"),
        }
    }
}

macro_rules! codes {
    ($category:ident; $(($name:ident, $value:literal, $docs:literal)),+ $(,)?) => {
        $(
            #[doc = $docs]
            pub const $name: Code = Code::new($value, Category::$category);
        )+
    };
}

codes!(Parse;
    (PARSE_SYNTAX, "", "A syntax error, which rustc reports without a code."),
);

codes!(Resolve;
    (RESOLVE_ENUM_PATTERN, "RAWR-RESOLVE-ENUM-PATTERN", "Invalid registered-enum switch pattern."),
    (RESOLVE_RECEIVER, "RAWR-RESOLVE-RECEIVER", "Invalid associated-method receiver grammar."),
    (RESOLVE_TYPE_SHAPE, "RAWR-RESOLVE-TYPE-SHAPE", "Invalid structural type grammar."),
    (RESOLVE_DUPLICATE_ITEM, "RAWR-RESOLVE-DUPLICATE-ITEM", "Duplicate source item."),
    (RESOLVE_UNKNOWN_TYPE, "E0425", "A type name nothing in scope declares, as rustc reports it."),
    (RESOLVE_UNKNOWN_IMPORT, "E0432", "A `use` of a path that doesn't exist, as rustc reports it."),
    (RESOLVE_INVALID_IMPORT, "RAWR-RESOLVE-INVALID-IMPORT", "Invalid compile-time import."),
    (RESOLVE_TYPE_ARGUMENTS, "RAWR-RESOLVE-TYPE-ARGUMENTS", "Invalid structural type arguments."),
    (RESOLVE_LOWERING, "RAWR-RESOLVE-LOWERING", "Unavailable item lowering."),
    (RESOLVE_DERIVE, "RAWR-RESOLVE-DERIVE", "Invalid derived capability."),
    (RESOLVE_DUPLICATE_MEMBER, "RAWR-RESOLVE-DUPLICATE-MEMBER", "Duplicate nominal member."),
    (RESOLVE_UNKNOWN_MEMBER, "RAWR-RESOLVE-UNKNOWN-MEMBER", "Unknown nominal member."),
    (RESOLVE_NO_METHOD, "E0599", "No method or associated item of that name, as rustc reports it."),
    (RESOLVE_NO_FIELD, "E0609", "No field of that name, as rustc reports it."),
    (RESOLVE_NOT_IN_MODULE, "E0425", "A path names no item of a module, as rustc reports it."),
    (RESOLVE_METHOD_VALUE, "E0615", "A method read like a field, as rustc reports it."),
    (RESOLVE_UNKNOWN_FUNCTION, "E0425", "A call of a name no function in scope has, as rustc reports it."),
    (SCRIPT_NO_FUNCTION, "E0425", "A typed handle names no function of the script, as rustc reports it."),
    (SCRIPT_SIGNATURE, "E0308", "A typed handle's signature differs from the script's, as rustc reports it."),
    (BORROW_MUTABLE_TWICE, "E0499", "A `&mut` borrow while another is live, as rustc reports it."),
    (BORROW_SHARED_MUTABLE, "E0502", "A shared and a `&mut` borrow live at once, as rustc reports it."),
    (BORROW_USE_MUTABLE, "E0503", "A use of a place while a `&mut` borrow of it is live, as rustc reports it."),
    (BORROW_MOVE, "E0505", "A move out of a place while a borrow of it is live, as rustc reports it."),
    (BORROW_MOVE_BEHIND, "E0507", "A move out of a place behind a reference, or out of an index, as rustc reports it."),
    (BORROW_MOVE_ELEMENT, "E0508", "A move out of an array's or a slice's element, as rustc reports it."),
    (BORROW_MOVE_DROP, "E0509", "A move out of a part of a value whose type implements `Drop`, as rustc reports it."),
    (BORROW_ASSIGN, "E0506", "An assignment to a place while a borrow of it is live, as rustc reports it."),
    (BORROW_LIFETIME, "E0597", "A local that ends while a borrow of it is live, as rustc reports it."),
    (BORROW_RETURNED_LOCAL, "E0515", "A function returns a borrow of its own local or a temporary, as rustc reports it."),
    (BORROW_TEMPORARY, "E0716", "A temporary value that ends while a borrow of it is live, as rustc reports it."),
    (BORROW_MISSING_LIFETIME, "E0106", "A reference's lifetime that elision gives nothing, as rustc reports it."),
    (BORROW_EXPLICIT_LIFETIME, "E0621", "A parameter whose elided lifetime a returned borrow needs to be the output's, as rustc reports it."),
    (BORROW_OUTLIVES, "RAWR-BORROW-OUTLIVES", "A returned borrow whose lifetime does not outlive the output's, rustc's \"lifetime may not live long enough\"."),
    (BORROW_CLOSURE_OUTLIVES, "E0373", "A closure that borrows a local and may outlive the function, as rustc reports it."),
    (BORROW_CLOSURE_UNIQUE, "E0500", "A closure's unique capture of a place while a borrow of it is live, as rustc reports it."),
    (BORROW_AFTER_UNIQUE, "E0501", "A borrow of a place while a closure's unique capture of it is live, as rustc reports it."),
    (BORROW_TWO_UNIQUE, "E0524", "Two closures' unique captures of a place live at once, as rustc reports it."),
    (BORROW_NOT_MUTABLE, "E0596", "A `&mut` borrow of a local not declared `mut`, as a `&mut self` method takes it, as rustc reports it."),
    (BORROW_ASSIGN_IMMUTABLE, "E0594", "An assignment through a place that isn't mutable, as a dereference of a type with no `DerefMut`, as rustc reports it."),
    (RESOLVE_UNKNOWN_VARIABLE, "E0425", "A name no value in scope has, as rustc reports it."),
    (RESOLVE_UNINITIALIZED, "E0381", "A local used before it is given a value, as rustc reports it."),
    (RESOLVE_MOVED_VALUE, "E0382", "A use of a value after it was moved, as rustc reports it."),
    (RESOLVE_CONSTRUCTION, "RAWR-RESOLVE-CONSTRUCTION", "Invalid nominal construction."),
    (RESOLVE_OVERFLOWING_LITERAL, "", "An integer literal its type can't hold, rustc's deny-by-default `overflowing_literals` lint, which has no code."),
    (RESOLVE_TYPE_MISMATCH, "E0308", "An expression whose type differs from the one expected, as rustc reports it."),
    (RESOLVE_IF_WITHOUT_ELSE, "E0317", "An `if` with no `else` where a value other than `()` is expected, as rustc reports it."),
    (RESOLVE_RETURN_UNIT, "E0069", "A `return;` in a function whose output isn't `()`, as rustc reports it."),
    (RESOLVE_IMMUTABLE_BINDING, "E0384", "An assignment to a variable not declared `mut`, as rustc reports it."),
    (RESOLVE_IMPL_OWNER, "RAWR-RESOLVE-IMPL-OWNER", "Invalid inherent implementation owner."),
    (RESOLVE_DUPLICATE_METHOD, "RAWR-RESOLVE-DUPLICATE-METHOD", "Duplicate inherent method."),
    (RESOLVE_METHOD_CALL, "RAWR-RESOLVE-METHOD-CALL", "Invalid inherent method call."),
    (RESOLVE_HOST_OPERATION, "RAWR-RESOLVE-HOST-OPERATION", "Invalid registered host operation."),
    (RESOLVE_UNKNOWN_MACRO, "", "A macro nothing in scope declares, which rustc reports without a code."),
    (RESOLVE_WRONG_KIND, "E0423", "A name used as another kind of item than the one it names, as rustc reports it."),
    (RESOLVE_UNUSED_VALUE, "RAWR-RESOLVE-UNUSED-VALUE", "Discarded value with required usage."),
    (RESOLVE_SELF, "RAWR-RESOLVE-SELF", "Invalid self use."),
    (RESOLVE_RECEIVER_MUTATION, "RAWR-RESOLVE-RECEIVER-MUTATION", "Mutation through a shared receiver."),
    (RESOLVE_METHOD_SIGNATURE, "RAWR-RESOLVE-METHOD-SIGNATURE", "Invalid method signature."),
    (RESOLVE_CAPABILITY, "E0277", "A type that doesn't implement a trait an operation needs, as rustc reports it."),
    (RESOLVE_MATCH, "RAWR-RESOLVE-MATCH", "Invalid nominal match."),
    (RESOLVE_UNSUPPORTED_SYNTAX, "RAWR-RESOLVE-UNSUPPORTED-SYNTAX", "A construct that isn't supported yet."),
    (RESOLVE_JUMP_IN_WHILE_CONDITION, "E0590", "A `break` or `continue` with no label in a `while` condition, as rustc reports it."),
    (RESOLVE_ASYNC_MAIN, "E0752", "An `async fn main` with no runtime's entry, as rustc reports it."),
    (RESOLVE_UNRESOLVED_IMPORT, "E0432", "A `use` of a path that doesn't exist, as rustc reports it."),
    (RESOLVE_FAILED_PATH, "E0433", "A path through a module or type that doesn't exist, as rustc reports it."),
    (RESOLVE_UNSTABLE, "E0658", "A use of an unstable library item, as rustc reports it."),
    (RESOLVE_MISSING_TRAIT_ITEMS, "E0046", "An impl that misses an item of its trait, as rustc reports it."),
    (RESOLVE_UNKNOWN_TRAIT, "E0405", "A trait name nothing in scope declares, as rustc reports it."),
    (RESOLVE_BINARY_OPERATOR, "E0369", "A binary operator whose left operand's type has no impl of its trait, as rustc reports it."),
    (RESOLVE_GENERIC_ARGUMENT_COUNT, "E0107", "A path with more or fewer generic arguments than its type takes, as rustc reports it."),
    (RESOLVE_EXPECTED_PATTERN, "E0532", "A pattern that names something no pattern is, as a call, as rustc reports it."),
    (RESOLVE_AMBIGUOUS_IMPL, "E0283", "A trait call whose type more than one impl could give, as rustc reports it."),
    (RESOLVE_NOT_YET, "RAWR-NOT-YET", "A std item rawr resolves but doesn't run yet."),
    (RESOLVE_PROJECTION_ANNOTATIONS, "E0284", "An operator whose output its operand's unknown type leaves open, as rustc reports it."),
    (RESOLVE_TYPE_ANNOTATIONS, "E0282", "A generic parameter whose type nothing gives, as rustc reports it."),
    (RESOLVE_DUPLICATE_GENERIC, "E0403", "A generic parameter name used twice in one item's generics, as rustc reports it."),
    (RESOLVE_PRIVATE_ITEM, "E0603", "A path to an item private to a module it is outside of, as rustc reports it."),
    (RESOLVE_PRIVATE_FUNCTION, "E0624", "A method or associated function private to a module the call is outside of, as rustc reports it."),
    (RESOLVE_ARGUMENT_COUNT, "E0061", "A call with more or fewer arguments than its function, method or constructor takes, as rustc reports it."),
    (RESOLVE_ARGUMENT_SIGNATURE, "E0631", "A closure or function whose parameters differ from those of the callable it is passed as, as rustc reports it."),
    (RESOLVE_CANNOT_COMPARE, "E0277", "An `==` or `!=` whose left operand's type has no `PartialEq` impl for the right operand's, as rustc reports it."),
    (RESOLVE_CLOSURE_ARGUMENT_COUNT, "E0057", "A call with more or fewer arguments than its closure takes, as rustc reports it."),
    (RESOLVE_MODULE_GRAPH, "RAWR-RESOLVE-MODULE-GRAPH", "Invalid static file-module graph."),
    (RESOLVE_PACKAGE_CYCLE, "RAWR-RESOLVE-PACKAGE-CYCLE", "Cyclic local package dependency."),
    (RESOLVE_PACKAGE_IDENTITY, "RAWR-RESOLVE-PACKAGE-IDENTITY", "Conflicting stable package identity."),
);

codes!(Framework;
    (FRAMEWORK_MISSING_ENTRY, "RAWR-FRAMEWORK-MISSING-ENTRY", "Missing application entry."),
    (FRAMEWORK_EXTENSION, "RAWR-FRAMEWORK-EXTENSION", "Missing or conflicting extension."),
    (FRAMEWORK_RUNTIME_BINDING, "RAWR-FRAMEWORK-RUNTIME-BINDING", "Invalid native runtime binding."),
    (FRAMEWORK_AUTHORITY, "RAWR-FRAMEWORK-AUTHORITY", "Invalid executable authority."),
    (FRAMEWORK_CONSTRUCTION, "RAWR-FRAMEWORK-CONSTRUCTION", "Invalid framework construction proof."),
    (FRAMEWORK_VIEW_OUTPUT, "RAWR-FRAMEWORK-VIEW-OUTPUT", "Invalid runtime view output."),
    (FRAMEWORK_THEME_OUTPUT, "RAWR-FRAMEWORK-THEME-OUTPUT", "Invalid runtime theme output."),
    (FRAMEWORK_TASK_OUTPUT, "RAWR-FRAMEWORK-TASK-OUTPUT", "Invalid runtime task output."),
    (FRAMEWORK_SUBSCRIPTION_OUTPUT, "RAWR-FRAMEWORK-SUBSCRIPTION-OUTPUT", "Invalid runtime subscription output."),
    (FRAMEWORK_NATIVE, "RAWR-FRAMEWORK-NATIVE", "Native framework failure."),
    (FRAMEWORK_SNAPSHOT, "RAWR-FRAMEWORK-SNAPSHOT", "Deterministic snapshot failure."),
);

codes!(Runtime;
    (RUNTIME_EVALUATION, "RAWR-RUNTIME-EVALUATION", "Script evaluation failure."),
    (RUNTIME_THROW, "RAWR-RUNTIME-THROW", "Script throw failure."),
    (RUNTIME_VIEW, "RAWR-RUNTIME-VIEW", "Application view failure."),
    (RUNTIME_UPDATE, "RAWR-RUNTIME-UPDATE", "Application update failure."),
);

codes!(Environment;
    (ENVIRONMENT_CURRENT_DIRECTORY, "RAWR-ENVIRONMENT-CURRENT-DIRECTORY", "Current-directory access failure."),
    (ENVIRONMENT_INPUT_NOT_FOUND, "RAWR-ENVIRONMENT-INPUT-NOT-FOUND", "Requested path does not exist."),
    (ENVIRONMENT_PROJECT_NOT_FOUND, "RAWR-ENVIRONMENT-PROJECT-NOT-FOUND", "Rawr project discovery failure."),
    (ENVIRONMENT_PROJECT_BUILD, "RAWR-ENVIRONMENT-PROJECT-BUILD", "Project builds differently from Rawr."),
    (ENVIRONMENT_PROJECT_AMBIGUOUS, "RAWR-ENVIRONMENT-AMBIGUOUS-PROJECT", "Ambiguous Rawr project discovery."),
    (ENVIRONMENT_UNREADABLE, "RAWR-ENVIRONMENT-UNREADABLE-RESOURCE", "Unreadable project resource."),
    (ENVIRONMENT_UNSUPPORTED_INPUT, "RAWR-ENVIRONMENT-UNSUPPORTED-INPUT", "Unsupported project input."),
    (ENVIRONMENT_MISSING_SOURCE, "RAWR-ENVIRONMENT-MISSING-SOURCE", "Missing project source."),
    (ENVIRONMENT_MISSING_CONFIG, "RAWR-ENVIRONMENT-MISSING-CONFIG", "Missing project manifest."),
    (ENVIRONMENT_ESCAPES_ROOT, "RAWR-ENVIRONMENT-ROOT-ESCAPE", "Project resource escapes its root."),
    (ENVIRONMENT_CREATE, "RAWR-ENVIRONMENT-PROJECT-CREATE", "Project creation failure."),
    (ENVIRONMENT_DOCTOR, "RAWR-ENVIRONMENT-DOCTOR", "Environment doctor failure."),
    (ENVIRONMENT_DIAGNOSTIC, "RAWR-ENVIRONMENT-DIAGNOSTIC", "Invalid diagnostic evidence."),
    (ENVIRONMENT_PACKAGE_MANIFEST, "RAWR-ENVIRONMENT-PACKAGE-MANIFEST", "Invalid Rawr package manifest."),
    (ENVIRONMENT_PACKAGE_RESOURCE, "RAWR-ENVIRONMENT-PACKAGE-RESOURCE", "Unreadable Rawr package resource."),
);

/// The code of a borrow error with rustc's code `rustc`.
pub(crate) fn borrow(rustc: &str) -> Code {
    match rustc {
        "E0499" => BORROW_MUTABLE_TWICE,
        "E0502" => BORROW_SHARED_MUTABLE,
        "E0503" => BORROW_USE_MUTABLE,
        "E0505" => BORROW_MOVE,
        "E0506" => BORROW_ASSIGN,
        "E0515" => BORROW_RETURNED_LOCAL,
        "E0716" => BORROW_TEMPORARY,
        "E0106" => BORROW_MISSING_LIFETIME,
        "E0621" => BORROW_EXPLICIT_LIFETIME,
        crate::resolve::borrowck::LIFETIME => BORROW_OUTLIVES,
        "E0373" => BORROW_CLOSURE_OUTLIVES,
        "E0500" => BORROW_CLOSURE_UNIQUE,
        "E0501" => BORROW_AFTER_UNIQUE,
        "E0524" => BORROW_TWO_UNIQUE,
        "E0596" => BORROW_NOT_MUTABLE,
        "E0594" => BORROW_ASSIGN_IMMUTABLE,
        "E0507" => BORROW_MOVE_BEHIND,
        "E0508" => BORROW_MOVE_ELEMENT,
        "E0509" => BORROW_MOVE_DROP,
        _ => BORROW_LIFETIME,
    }
}

/// The code of a private item's error: E0624 for an inherent impl's
/// function, E0603 for an item a path names.
pub(crate) fn private(item: crate::resolve::Private) -> Code {
    match item.code() {
        "E0624" => RESOLVE_PRIVATE_FUNCTION,
        _ => RESOLVE_PRIVATE_ITEM,
    }
}

/// The code of rustc's error `rustc` that `resolve::Error::StdPath` carries:
/// a std path's (`resolve::std_path`), or another error in rustc's words.
/// The code of a local whose type is unknown where rustc needs it.
pub(crate) fn type_needed(rustc: &str) -> Code {
    match rustc {
        "E0283" => RESOLVE_AMBIGUOUS_IMPL,
        "E0284" => RESOLVE_PROJECTION_ANNOTATIONS,
        _ => RESOLVE_TYPE_ANNOTATIONS,
    }
}

pub(crate) fn std_path(rustc: &str) -> Code {
    match rustc {
        "E0432" => RESOLVE_UNRESOLVED_IMPORT,
        "E0433" => RESOLVE_FAILED_PATH,
        "E0425" => RESOLVE_NOT_IN_MODULE,
        "E0046" => RESOLVE_MISSING_TRAIT_ITEMS,
        "E0405" => RESOLVE_UNKNOWN_TRAIT,
        "E0369" => RESOLVE_BINARY_OPERATOR,
        "E0283" => RESOLVE_AMBIGUOUS_IMPL,
        "E0532" => RESOLVE_EXPECTED_PATTERN,
        "E0107" => RESOLVE_GENERIC_ARGUMENT_COUNT,
        _ => RESOLVE_UNSTABLE,
    }
}
