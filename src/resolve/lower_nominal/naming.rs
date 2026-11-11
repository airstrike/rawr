//! The names of generated functions, dispatchers and placeholders in the
//! lowered text, and how a generated name reads in a diagnostic.

use crate::ast::NOMINAL_MATCH_SENTINEL;
use crate::resolve::type_ref;
use crate::{method, ImmutableString};

pub(super) const SENTINEL: &str = "__rawr_nominal_op_";
pub(super) const METHOD_SENTINEL: &str = "__rawr_nominal_method_";
pub(super) const FUNCTION_SENTINEL: &str = "__rawr_nominal_function_";
/// A rewrite's target for a script function's own name where the lowered
/// text calls something else by it: the call reaches only the runtime's
/// functions (`rewrite_call`).
pub(super) const NATIVE_ONLY: &str = "\0rawr.native-only";
pub(super) const DISPATCH_SENTINEL: &str = "__rawr_nominal_dispatch_";
/// The program's dispatcher of `Debug::fmt`, present when it has a
/// hand-written `Debug` impl; it falls back to [`DEBUG_GENERIC`].
pub(crate) const DEBUG_DISPATCHER: &str = "__rawr_nominal_dispatch_Debug__fmt";
/// The `Debug` text of any value: a derive's, a builtin's or a crate
/// type's. Present when the program formats with `Debug` or a debugger is
/// registered while it compiles.
pub(crate) const DEBUG_GENERIC: &str = "\0rawr.generic.debug";
/// The generated function from a type witness to that type's default.
pub(super) const DEFAULT_DISPATCHER: &str = "__rawr_nominal_default";
pub(super) const CLOSURE_SENTINEL: &str = "__rawr_nominal_closure_";
pub(super) const PRIVATE_PREFIX: &str = "\0rawr.nominal.";

/// A method's generated name, by its symbol: a lowering that calls it
/// names it the same in every generation of the program, however the
/// program's methods are numbered (#1718).
pub(super) fn method_name(definition: &method::Definition) -> String {
    stable(METHOD_SENTINEL, &definition.symbol())
}

/// A function's generated name, by its key, as `method_name`.
pub(super) fn function_name(key: &crate::item::Key) -> String {
    stable(FUNCTION_SENTINEL, &private_function_name(key))
}

/// `sentinel`, then the first `DIGITS` digits of `symbol`'s digest in
/// base 62: an identifier no other symbol's reaches but by a 71-bit
/// collision. It fits a conversion's placeholder (`conversion_placeholder`).
fn stable(sentinel: &str, symbol: &str) -> String {
    use sha2::Digest as _;
    const DIGITS: usize = 12;
    const ALPHABET: &[u8; 62] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let digest = sha2::Sha256::digest(symbol.as_bytes());
    let mut name = String::with_capacity(sentinel.len() + DIGITS);
    name.push_str(sentinel);
    name.extend(
        digest[..DIGITS]
            .iter()
            .map(|byte| char::from(ALPHABET[usize::from(*byte) % ALPHABET.len()])),
    );
    name
}

/// A private function's generated name: its path, and a block item's
/// place among its module's block items of its name, which no user reads
/// (`function_display_name`).
pub(crate) fn private_function_name(key: &crate::item::Key) -> ImmutableString {
    match key.block() {
        Some(block) => format!("{PRIVATE_FUNCTION}{key}{BLOCK_ITEM}{block}").into(),
        None => format!("{PRIVATE_FUNCTION}{key}").into(),
    }
}

pub(super) const PRIVATE_FUNCTION: &str = "\0rawr.function.";

/// What separates a block item's place from its path in its generated name.
const BLOCK_ITEM: char = '\0';

/// A function's name as a user reads it: a private function's path in its
/// crate (`main::geometry::area`), and any other name as it is.
pub(crate) fn function_display_name(name: &str) -> &str {
    match name.strip_prefix(PRIVATE_FUNCTION) {
        Some(path) => path.split(BLOCK_ITEM).next().unwrap_or(path),
        None => name,
    }
}

pub(super) fn private_method_name(definition: &method::Definition) -> String {
    definition.symbol().into()
}

/// The placeholder of the pattern numbered `index` after `numbering`, its
/// source's part (`Parser::numbering`).
pub(super) fn pattern_name(numbering: &str, index: usize) -> String {
    format!("{NOMINAL_MATCH_SENTINEL}{numbering}{index}")
}

/// The placeholder of the `text.into()` numbered `index`.
pub(super) fn text_into_placeholder(index: usize) -> String {
    // One more than any helper name, for the `(` written after it.
    let head = format!("__rawr_nominal_text_into_{index:010}");
    let width = head.len() + 1;
    format!("{head:_<width$}")
}

/// A trait's name as a generated name holds it: a trait known by its path,
/// `home::IntoContent`, as `home__IntoContent`.
fn identifier(trait_name: &str) -> std::borrow::Cow<'_, str> {
    if trait_name.contains("::") {
        trait_name.replace("::", "__").into()
    } else {
        trait_name.into()
    }
}

/// The text of a generic trait's argument list: `i64, String`.
pub(super) fn instance_text(arguments: &[type_ref::Type]) -> String {
    arguments
        .iter()
        .map(crate::resolve::full_type_name)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The dispatcher of `trait_name::method` for the trait's instance
/// `index` of a generic trait; `FALLBACK_INSTANCE` dispatches by the
/// receiver's type alone. Same length as `instance_placeholder`.
pub(super) fn instance_dispatcher_name(trait_name: &str, method: &str, index: usize) -> String {
    format!(
        "{DISPATCH_SENTINEL}{}__{method}__{index:08}",
        identifier(trait_name)
    )
}

/// A call's dispatcher before inference fixes its trait arguments.
pub(super) fn instance_placeholder(trait_name: &str, method: &str, pending: usize) -> String {
    format!(
        "{DISPATCH_SENTINEL}{}__{method}__p{pending:07}",
        identifier(trait_name)
    )
}

pub(super) const FALLBACK_INSTANCE: usize = 99_999_999;

/// The generated function that runs `trait_name::function`, a function
/// without `self`, for the type a witness names.
pub(super) fn static_dispatcher_name(trait_name: &str, function: &str) -> String {
    format!(
        "{DISPATCH_SENTINEL}static_{}__{function}",
        identifier(trait_name)
    )
}

/// The static dispatcher of a generic trait's function without `self` for
/// the trait's instance `index`, as `instance_dispatcher_name`.
pub(super) fn static_instance_dispatcher_name(
    trait_name: &str,
    function: &str,
    index: usize,
) -> String {
    format!(
        "{DISPATCH_SENTINEL}static_{}__{function}__{index:08}",
        identifier(trait_name)
    )
}

/// A static call's dispatcher before inference fixes its trait arguments.
pub(super) fn static_instance_placeholder(
    trait_name: &str,
    function: &str,
    pending: usize,
) -> String {
    format!(
        "{DISPATCH_SENTINEL}static_{}__{function}__p{pending:07}",
        identifier(trait_name)
    )
}

/// The hidden parameter carrying the witness of the generic `name`.
pub(super) fn witness_variable(name: &str) -> String {
    format!("__rawr_witness_{name}")
}

/// A placeholder long enough for any witness it becomes.
pub(super) fn witness_placeholder(index: usize) -> String {
    format!("__rawr_nominal_witness_{index:024}")
}

/// The generated function that runs `trait_name::method` for a value by its
/// runtime type.
pub(super) fn trait_dispatcher_name(trait_name: &str, method: &str) -> String {
    format!("{DISPATCH_SENTINEL}{}__{method}", identifier(trait_name))
}

/// A placeholder before the text of a pending `text.into()`, long enough
/// for the helper that marks the text lent at `'static`, which it becomes
/// where the impl the use selects borrows the text, and blank elsewhere.
pub(super) fn lent_text_placeholder(index: usize) -> String {
    format!("__rawr_nominal_lent_text_{index:010}")
}

/// A placeholder long enough for any method helper name it becomes.
pub(super) fn conversion_placeholder(index: usize) -> String {
    format!("__rawr_nominal_conversion_{index:010}")
}
