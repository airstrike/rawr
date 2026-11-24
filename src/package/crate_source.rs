//! Crate modules whose generic code rawr runs from the crate's own source,
//! as rustc compiles a crate's generic code again for each program that
//! instantiates it. The rest of such a module stays native, as rustc
//! compiles it once in the crate's own library: its concrete functions,
//! its concrete types and their impls, and its statics.
//!
//! Each such module is a module of the program too ([`name`]): the
//! crate's file with only its generic items and its `use` declarations
//! left, at their places, so positions stay the file's, and a public glob
//! import of the native module after them ([`generic::text`]). A generic
//! body reads the module's concrete items through that glob, which its
//! local items shadow, as rustc's do. The runner's copy of the crate makes
//! each item and field of such a module public ([`opened`]), so the glob
//! reaches its private ones too. The program module stands for the
//! crate's: every path into the crate module from outside it is a path
//! into the program module (`resolve::package`), so a path to a generic
//! item is the program's item and a path to a concrete one goes on through
//! the glob to the crate's.
//!
//! An item is generic when it has type or const parameters, as an
//! argument-position `impl Trait` gives it one, or when it is an impl of a
//! generic type the module keeps: its methods act on the type rawr runs.
//! A function or type that has only lifetime parameters is concrete.
//!
//! The modules are named by [`VARIABLE`]: `events::tinted,iced::widget`,
//! each path starting with the crate's name as the program writes it.

use crate::ImmutableString;

pub(crate) mod generic;
#[cfg(all(feature = "rust-import", not(target_arch = "wasm32")))]
pub(crate) mod opened;

/// The variable that names the crate modules rawr reads generic code from.
pub(crate) const VARIABLE: &str = "RAWR_CRATE_SOURCE";

/// The prefix of a program module that holds a crate module's generic items.
const PREFIX: &str = "__rawr_crate";

/// The prefix of the trait that holds the generic inherent methods a
/// crate-source module gives one of its crate's own concrete types: rustc
/// lets no other crate add inherent methods to a type, so the program
/// module holds them as this trait's impl, which is in scope wherever the
/// type is, as inherent methods are.
const INHERENT: &str = "__rawr_inherent__";

/// Whether `trait_name` holds a crate-source module's inherent methods.
pub(crate) fn is_inherent(trait_name: &str) -> bool {
    trait_name.starts_with(INHERENT)
}

/// The trait that holds the generic inherent methods of `owner`, a type's
/// name, which a crate-source module gives it.
pub(crate) fn inherent(owner: &str) -> String {
    format!("{INHERENT}{owner}")
}

/// The crate modules [`VARIABLE`] names, each as its path from the crate's
/// name.
pub(crate) fn modules() -> Vec<Box<[ImmutableString]>> {
    let Some(named) = std::env::var_os(VARIABLE) else {
        return Vec::new();
    };
    named
        .to_string_lossy()
        .split(',')
        .map(str::trim)
        .filter(|path| path.contains("::"))
        .map(|path| path.split("::").map(ImmutableString::from).collect())
        .collect()
}

/// The crates whose modules [`VARIABLE`] names, by the names the program
/// writes them by.
#[cfg(all(feature = "rust-import", not(target_arch = "wasm32")))]
pub(crate) fn crates() -> Vec<ImmutableString> {
    let mut crates = modules()
        .into_iter()
        .filter_map(|path| path.first().cloned())
        .collect::<Vec<_>>();
    crates.sort();
    crates.dedup();
    crates
}

/// Whether `module` is the name of a program module that holds a crate
/// module's generic items ([`name`]).
pub(crate) fn is_name(module: &str) -> bool {
    module
        .strip_prefix(PREFIX)
        .is_some_and(|rest| rest.starts_with("__"))
}

/// The program module that holds the generic items of the crate module at
/// `path` (`events::tinted` is `__rawr_crate__events__tinted`).
pub(crate) fn name<S: AsRef<str>>(path: &[S]) -> ImmutableString {
    let mut name = String::from(PREFIX);
    for segment in path {
        name.push_str("__");
        name.push_str(segment.as_ref());
    }
    name.into()
}
