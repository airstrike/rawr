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

    /// Return the stable printable code.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.value
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

macro_rules! codes {
    ($category:ident; $(($name:ident, $value:literal, $docs:literal)),+ $(,)?) => {
        $(
            #[doc = $docs]
            pub const $name: Code = Code::new($value, Category::$category);
        )+
    };
}

codes!(Parse;
    (PARSE_SYNTAX, "RAWR-PARSE-SYNTAX", "General source grammar failure."),
    (PARSE_BAD_INPUT, "RAWR-PARSE-BAD-INPUT", "Invalid source token or literal."),
    (PARSE_UNEXPECTED_EOF, "RAWR-PARSE-UNEXPECTED-EOF", "Unexpected end of source."),
    (PARSE_MISSING_TOKEN, "RAWR-PARSE-MISSING-TOKEN", "Required source token is missing."),
    (PARSE_MISSING_SYMBOL, "RAWR-PARSE-MISSING-SYMBOL", "Required source symbol is missing."),
    (PARSE_UNEXPECTED, "RAWR-PARSE-UNEXPECTED", "Syntax Rust does not have, as rustc reports it."),
);

codes!(Resolve;
    (RESOLVE_ENUM_PATTERN, "RAWR-RESOLVE-ENUM-PATTERN", "Invalid registered-enum switch pattern."),
    (RESOLVE_RECEIVER, "RAWR-RESOLVE-RECEIVER", "Invalid associated-method receiver grammar."),
    (RESOLVE_TYPE_SHAPE, "RAWR-RESOLVE-TYPE-SHAPE", "Invalid structural type grammar."),
    (RESOLVE_DUPLICATE_ITEM, "RAWR-RESOLVE-DUPLICATE-ITEM", "Duplicate source item."),
    (RESOLVE_UNKNOWN_TYPE, "RAWR-RESOLVE-UNKNOWN-TYPE", "Unknown source type."),
    (RESOLVE_UNKNOWN_IMPORT, "RAWR-RESOLVE-UNKNOWN-IMPORT", "Unknown compile-time import."),
    (RESOLVE_INVALID_IMPORT, "RAWR-RESOLVE-INVALID-IMPORT", "Invalid compile-time import."),
    (RESOLVE_TYPE_ARGUMENTS, "RAWR-RESOLVE-TYPE-ARGUMENTS", "Invalid structural type arguments."),
    (RESOLVE_LOWERING, "RAWR-RESOLVE-LOWERING", "Unavailable item lowering."),
    (RESOLVE_DERIVE, "RAWR-RESOLVE-DERIVE", "Invalid derived capability."),
    (RESOLVE_DUPLICATE_MEMBER, "RAWR-RESOLVE-DUPLICATE-MEMBER", "Duplicate nominal member."),
    (RESOLVE_UNKNOWN_MEMBER, "RAWR-RESOLVE-UNKNOWN-MEMBER", "Unknown nominal member."),
    (RESOLVE_NO_METHOD, "E0599", "No method or associated item of that name, as rustc reports it."),
    (RESOLVE_NO_FIELD, "E0609", "No field of that name, as rustc reports it."),
    (RESOLVE_METHOD_VALUE, "E0615", "A method read like a field, as rustc reports it."),
    (RESOLVE_UNKNOWN_FUNCTION, "RAWR-RESOLVE-UNKNOWN-FUNCTION", "Unknown function."),
    (SCRIPT_NO_FUNCTION, "E0425", "A typed handle names no function of the script, as rustc reports it."),
    (SCRIPT_SIGNATURE, "E0308", "A typed handle's signature differs from the script's, as rustc reports it."),
    (BORROW_MUTABLE_TWICE, "E0499", "A `&mut` borrow while another is live, as rustc reports it."),
    (BORROW_SHARED_MUTABLE, "E0502", "A shared and a `&mut` borrow live at once, as rustc reports it."),
    (BORROW_USE_MUTABLE, "E0503", "A use of a place while a `&mut` borrow of it is live, as rustc reports it."),
    (BORROW_MOVE, "E0505", "A move out of a place while a borrow of it is live, as rustc reports it."),
    (BORROW_ASSIGN, "E0506", "An assignment to a place while a borrow of it is live, as rustc reports it."),
    (BORROW_LIFETIME, "E0597", "A local that ends while a borrow of it is live, as rustc reports it."),
    (BORROW_CLOSURE_OUTLIVES, "E0373", "A closure that borrows a local and may outlive the function, as rustc reports it."),
    (BORROW_CLOSURE_UNIQUE, "E0500", "A closure's unique capture of a place while a borrow of it is live, as rustc reports it."),
    (BORROW_AFTER_UNIQUE, "E0501", "A borrow of a place while a closure's unique capture of it is live, as rustc reports it."),
    (BORROW_TWO_UNIQUE, "E0524", "Two closures' unique captures of a place live at once, as rustc reports it."),
    (RESOLVE_UNKNOWN_VARIABLE, "RAWR-RESOLVE-UNKNOWN-VARIABLE", "Unknown variable."),
    (RESOLVE_MOVED_VALUE, "E0382", "A use of a value after it was moved, as rustc reports it."),
    (RESOLVE_CONSTRUCTION, "RAWR-RESOLVE-CONSTRUCTION", "Invalid nominal construction."),
    (RESOLVE_TYPE_MISMATCH, "RAWR-RESOLVE-TYPE-MISMATCH", "Resolved type mismatch."),
    (RESOLVE_IMMUTABLE_BINDING, "RAWR-RESOLVE-IMMUTABLE-BINDING", "Mutation of an immutable binding."),
    (RESOLVE_IMPL_OWNER, "RAWR-RESOLVE-IMPL-OWNER", "Invalid inherent implementation owner."),
    (RESOLVE_DUPLICATE_METHOD, "RAWR-RESOLVE-DUPLICATE-METHOD", "Duplicate inherent method."),
    (RESOLVE_METHOD_CALL, "RAWR-RESOLVE-METHOD-CALL", "Invalid inherent method call."),
    (RESOLVE_HOST_OPERATION, "RAWR-RESOLVE-HOST-OPERATION", "Invalid registered host operation."),
    (RESOLVE_UNUSED_VALUE, "RAWR-RESOLVE-UNUSED-VALUE", "Discarded value with required usage."),
    (RESOLVE_SELF, "RAWR-RESOLVE-SELF", "Invalid self use."),
    (RESOLVE_RECEIVER_MUTATION, "RAWR-RESOLVE-RECEIVER-MUTATION", "Mutation through a shared receiver."),
    (RESOLVE_METHOD_SIGNATURE, "RAWR-RESOLVE-METHOD-SIGNATURE", "Invalid method signature."),
    (RESOLVE_CAPABILITY, "RAWR-RESOLVE-CAPABILITY", "Missing proven capability."),
    (RESOLVE_MATCH, "RAWR-RESOLVE-MATCH", "Invalid nominal match."),
    (RESOLVE_UNSUPPORTED_SYNTAX, "RAWR-RESOLVE-UNSUPPORTED-SYNTAX", "Unsupported resolved syntax."),
    (RESOLVE_PREVIEW, "RAWR-RESOLVE-PREVIEW", "Invalid local preview metadata."),
    (RESOLVE_WIDGET, "RAWR-RESOLVE-WIDGET", "Invalid scripted-widget lifecycle protocol."),
    (RESOLVE_MODULE_GRAPH, "RAWR-RESOLVE-MODULE-GRAPH", "Invalid static file-module graph."),
    (RESOLVE_PACKAGE_CYCLE, "RAWR-RESOLVE-PACKAGE-CYCLE", "Cyclic local package dependency."),
    (RESOLVE_PACKAGE_IDENTITY, "RAWR-RESOLVE-PACKAGE-IDENTITY", "Conflicting stable package identity."),
);

codes!(Framework;
    (FRAMEWORK_MISSING_ENTRY, "RAWR-FRAMEWORK-MISSING-ENTRY", "Missing application entry."),
    (FRAMEWORK_INVALID_MAIN, "RAWR-FRAMEWORK-INVALID-MAIN", "Invalid application main."),
    (FRAMEWORK_MULTIPLE_ENTRIES, "RAWR-FRAMEWORK-MULTIPLE-ENTRIES", "Multiple application entries."),
    (FRAMEWORK_CONDITIONAL_ENTRY, "RAWR-FRAMEWORK-CONDITIONAL-ENTRY", "Conditional application entry."),
    (FRAMEWORK_NESTED_ENTRY, "RAWR-FRAMEWORK-NESTED-ENTRY", "Nested application entry."),
    (FRAMEWORK_HIDDEN_ENTRY, "RAWR-FRAMEWORK-HIDDEN-ENTRY", "Hidden application entry."),
    (FRAMEWORK_NON_TAIL_ENTRY, "RAWR-FRAMEWORK-NON-TAIL-ENTRY", "Non-tail application entry."),
    (FRAMEWORK_MALFORMED_ENTRY, "RAWR-FRAMEWORK-MALFORMED-ENTRY", "Malformed application entry."),
    (FRAMEWORK_NON_SYMBOLIC_CALLBACK, "RAWR-FRAMEWORK-NON-SYMBOLIC-CALLBACK", "Non-symbolic application callback."),
    (FRAMEWORK_UNKNOWN_CALLBACK, "RAWR-FRAMEWORK-UNKNOWN-CALLBACK", "Unknown application callback."),
    (FRAMEWORK_CALLBACK_KIND, "RAWR-FRAMEWORK-CALLBACK-KIND", "Invalid application callback kind."),
    (FRAMEWORK_CALLBACK_RECEIVER, "RAWR-FRAMEWORK-CALLBACK-RECEIVER", "Invalid application callback receiver."),
    (FRAMEWORK_CALLBACK_ARITY, "RAWR-FRAMEWORK-CALLBACK-ARITY", "Invalid application callback arity."),
    (FRAMEWORK_TYPE_MISMATCH, "RAWR-FRAMEWORK-TYPE-MISMATCH", "Application type mismatch."),
    (FRAMEWORK_INVALID_OUTPUT, "RAWR-FRAMEWORK-INVALID-OUTPUT", "Invalid application callback output."),
    (FRAMEWORK_MISSING_DEFAULT, "RAWR-FRAMEWORK-MISSING-DEFAULT", "Missing application Default capability."),
    (FRAMEWORK_CAPABILITY, "RAWR-FRAMEWORK-CAPABILITY", "Missing application capability."),
    (FRAMEWORK_FACADE, "RAWR-FRAMEWORK-FACADE", "Invalid framework facade contract."),
    (FRAMEWORK_FACADE_DEPENDENCY, "RAWR-FRAMEWORK-FACADE-DEPENDENCY", "Conflicting framework facade dependency."),
    (FRAMEWORK_RUNTIME_TYPE, "RAWR-FRAMEWORK-RUNTIME-TYPE", "Unsupported framework runtime type."),
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
    (RUNTIME_VALUE, "RAWR-RUNTIME-VALUE", "Runtime application value failure."),
    (RUNTIME_BOOT, "RAWR-RUNTIME-BOOT", "Application boot failure."),
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
        "E0373" => BORROW_CLOSURE_OUTLIVES,
        "E0500" => BORROW_CLOSURE_UNIQUE,
        "E0501" => BORROW_AFTER_UNIQUE,
        "E0524" => BORROW_TWO_UNIQUE,
        _ => BORROW_LIFETIME,
    }
}
