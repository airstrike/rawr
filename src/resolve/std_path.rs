//! std, core and alloc paths as rustc resolves them, from the std table
//! (`crate::std_table`, D711 step 1).
//!
//! A path into std resolves when std has it, through every re-export and
//! glob, and is refused in rustc's words when it doesn't (E0432, E0433,
//! E0425) or names an unstable item (E0658). An item std has and rawr
//! can't run yet fails where it is used, with "rawr doesn't run `X` yet"
//! (D712.1).
// Shadow mode (D711): only the shadow tests read it until the switch.
#![allow(dead_code)]

use super::Error;
use crate::std_table::{self, Missing, Namespace, Resolved, Stability};
use crate::{ImmutableString, Span};

/// The std types rawr represents with its own, by definition, and rawr's
/// name for each: D711's binding table for types. A std type that isn't
/// here resolves, and is "not yet" where it is used.
pub(crate) const TYPES: &[(&str, &str)] = &[
    ("core::option::Option", "Option"),
    ("core::result::Result", "Result"),
    ("alloc::vec::Vec", "Vec"),
    ("alloc::boxed::Box", "Box"),
    ("alloc::string::String", "String"),
    ("alloc::rc::Rc", "Rc"),
    ("alloc::sync::Arc", "Arc"),
    ("std::collections::hash::map::HashMap", "HashMap"),
    ("std::collections::hash::set::HashSet", "HashSet"),
    ("alloc::collections::btree::map::BTreeMap", "BTreeMap"),
    ("alloc::collections::btree::set::BTreeSet", "BTreeSet"),
    // Rawr's `Vec` (`vec_deque_as_vec`).
    ("alloc::collections::vec_deque::VecDeque", "Vec"),
    ("core::cmp::Reverse", "Reverse"),
    ("core::cmp::Ordering", "Ordering"),
    ("core::cell::RefCell", "RefCell"),
    ("core::cell::Cell", "Cell"),
    ("core::ops::range::RangeInclusive", "RangeInclusive"),
    ("core::ops::range::Range", crate::item::type_ref::ITER_TYPE),
    ("core::marker::PhantomData", "PhantomData"),
    ("core::time::Duration", "Duration"),
    ("std::time::Instant", "Instant"),
    ("core::num::error::ParseIntError", "ParseIntError"),
    ("core::num::float_parse::ParseFloatError", "ParseFloatError"),
    ("core::str::error::ParseBoolError", "ParseBoolError"),
    ("core::char::convert::ParseCharError", "ParseCharError"),
    ("core::convert::Infallible", "Infallible"),
    ("alloc::string::FromUtf8Error", "FromUtf8Error"),
    ("core::num::error::TryFromIntError", "TryFromIntError"),
    ("core::fmt::Formatter", "String"),
    ("core::fmt::Result", "Result"),
];

/// The std definition rawr's builtin type `name` is, as `alloc::vec::Vec`
/// for `Vec`.
pub(crate) fn definition_of(name: &str) -> Option<&'static str> {
    TYPES
        .iter()
        .find(|(definition, rawr)| *rawr == name && !definition.contains("VecDeque"))
        .map(|(definition, _)| *definition)
}

/// Whether rawr represents the std type `definition`.
pub(crate) fn represented(definition: &str) -> bool {
    TYPES.iter().any(|(known, _)| *known == definition)
}

/// The type arity a path to rawr's builtin `name` may write, from std's
/// declaration: its parameters with no default, and every one stable Rust
/// may name. `HashMap` is `(2, 3)`: `K`, `V` and `S = RandomState`.
pub(crate) fn arity(name: &str) -> Option<(usize, usize)> {
    std_table::item(definition_of(name)?).map(|item| item.type_arity())
}

/// The unstable parameter a type argument at `index` of rawr's builtin
/// `name` would fill, as `Vec`'s second fills `A` (`allocator_api`).
pub(crate) fn unstable_argument(name: &str, index: usize) -> Option<&'static str> {
    std_table::item(definition_of(name)?)?
        .generics()
        .filter(|parameter| parameter.kind == std_table::ParameterKind::Type)
        .nth(index)?
        .unstable
}

fn written(segments: &[&str]) -> String {
    segments.join("::")
}

/// rustc's E0658 for naming `path`, unstable with `feature`.
pub(crate) fn unstable(stability: Stability, reference: Span) -> Option<Error> {
    let Stability::Unstable {
        feature,
        issue,
        reason,
    } = stability
    else {
        return None;
    };
    let message = match reason {
        Some(reason) => format!("use of unstable library feature `{feature}`: {reason}"),
        None => format!("use of unstable library feature `{feature}`"),
    };
    Some(Error::StdPath {
        code: "E0658",
        message: message.into(),
        label: None,
        note: issue.map(|issue| {
            format!(
                "see issue #{issue} <https://github.com/rust-lang/rust/issues/{issue}> for more information"
            )
            .into()
        }),
        reference,
    })
}

/// A `use` of `path`. `Ok(None)` when it isn't a path into std, core or
/// alloc; else what it names, or rustc's E0432 (std has no such path) or
/// E0658 (an unstable item).
pub(crate) fn import(path: &str, reference: Span) -> Result<Option<Resolved>, Error> {
    let segments = path.split("::").collect::<Vec<_>>();
    if !segments
        .first()
        .is_some_and(|root| std_table::is_root(root))
    {
        return Ok(None);
    }
    // `use std;` and `use std::{..}` name the crate.
    if segments.len() == 1 {
        return Ok(std_table::any(path).map(Resolved::Path));
    }
    match std_table::resolve(&segments, Namespace::Type) {
        Ok(resolved) => match unstable(resolved.stability(), reference) {
            Some(error) => Err(error),
            None => Ok(Some(resolved)),
        },
        Err(missing) => Err(unresolved_import(&missing, reference)),
    }
}

/// rustc's E0432 for a `use` std doesn't have.
fn unresolved_import(missing: &Missing, reference: Span) -> Error {
    let parent = missing.found.last().map_or("", String::as_str);
    let mut through = missing.found.clone();
    through.push(missing.segment.clone());
    let label = if missing.last {
        format!(
            "no `{}` in `{}`",
            missing.segment,
            written_path(&missing.found)
        )
    } else {
        format!("could not find `{}` in `{parent}`", missing.segment)
    };
    Error::StdPath {
        code: "E0432",
        message: format!("unresolved import `{}`", through.join("::")).into(),
        label: Some(label.into()),
        note: None,
        reference,
    }
}

/// rustc's "no `X` in `a::b`" names the module by its last segment when the
/// path has one, as in `no `Nope` in `collections``.
fn written_path(found: &[String]) -> &str {
    found.last().map_or("", String::as_str)
}

/// A path into std written in a type or an expression, as
/// `core::time::Instant` or `std::thread::sleep`: what it names, or
/// rustc's error for it (E0433 through a module std doesn't have, E0425
/// for an item it doesn't have, E0658 for an unstable one). `kind` is what
/// the use reads its last segment as: `type`, `function` or `value`.
pub(crate) fn check(
    segments: &[&str],
    namespace: Namespace,
    kind: &str,
    reference: Span,
) -> Result<Resolved, Error> {
    match std_table::resolve(segments, namespace) {
        Ok(resolved) => match unstable(resolved.stability(), reference) {
            Some(error) => Err(error),
            None => Ok(resolved),
        },
        Err(missing) if missing.last && !missing.found.is_empty() => {
            let module = missing.found.join("::");
            Err(Error::StdPath {
                code: "E0425",
                message: format!(
                    "cannot find {kind} `{}` in module `{module}`",
                    missing.segment
                )
                .into(),
                label: Some(format!("not found in `{module}`").into()),
                note: None,
                reference,
            })
        }
        Err(missing) => {
            let parent = missing.found.last().map_or("", String::as_str);
            Err(Error::StdPath {
                code: "E0433",
                message: format!("cannot find `{}` in `{parent}`", missing.segment).into(),
                label: Some(format!("could not find `{}` in `{parent}`", missing.segment).into()),
                note: None,
                reference,
            })
        }
    }
}

/// The names `use module::*;` binds when `module` is a std module: every
/// public name of it, in every namespace. `None` for another path.
pub(crate) fn glob(module: &str) -> Option<Vec<ImmutableString>> {
    if !module.split("::").next().is_some_and(std_table::is_root) || !std_table::is_module(module) {
        return None;
    }
    let mut names = std_table::children(module)
        .map(|child| &child.path[module.len() + 2..])
        .map(ImmutableString::from)
        .collect::<Vec<_>>();
    names.dedup();
    Some(names)
}

/// rawr's error for a use of the std path `path` it resolves but can't run
/// yet, or E0658 for an unstable one. `None` when std has no such path.
pub(crate) fn not_yet(path: &str, namespace: Namespace, reference: Span) -> Option<Error> {
    let segments = path.split("::").collect::<Vec<_>>();
    if !segments
        .first()
        .is_some_and(|root| std_table::is_root(root))
        || segments.len() < 2
    {
        return None;
    }
    let resolved = std_table::resolve(&segments, namespace).ok()?;
    if let Some(error) = unstable(resolved.stability(), reference) {
        return Some(error);
    }
    // A module is no use on its own.
    if matches!(resolved, Resolved::Path(path) if path.kind == "mod") {
        return None;
    }
    Some(Error::NotYet {
        path: written(&segments).into(),
        reference,
    })
}

/// The std trait a bound or an impl names by its path, as
/// `std::fmt::Display` or `fmt::Debug` (after `use std::fmt;`): its name,
/// when it is one of rawr's builtin traits.
pub(crate) fn trait_path(segments: &[crate::item::type_ref::Segment]) -> Option<crate::item::Name> {
    let [prefix @ .., last] = segments else {
        return None;
    };
    if prefix.is_empty()
        || !last.arguments.is_empty()
        || !super::BUILTIN_TRAITS.contains(&last.name.text.as_str())
        || prefix.iter().any(|module| !module.arguments.is_empty())
    {
        return None;
    }
    let mut written = prefix
        .iter()
        .chain([last])
        .map(|segment| segment.name.text.as_str())
        .collect::<Vec<_>>();
    // A path from a std module a `use` brought in: `fmt::Display`.
    if !std_table::is_root(written[0]) {
        written.insert(0, "std");
    }
    match std_table::resolve(&written, Namespace::Type) {
        Ok(Resolved::Path(path)) if path.kind == "trait" && path.stability == Stability::Stable => {
            Some(last.name.clone())
        }
        _ => None,
    }
}
