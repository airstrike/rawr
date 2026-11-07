//! std's traits from the std table (D711 step 2): a program implements and
//! bounds by any stable std trait, whose declaration is std's own, read as
//! a program's trait is.
//!
//! A trait's declaration lists its required items, with the signatures std
//! declares them with. A provided function is std's to run: an impl may
//! override it, and a call rawr can't run yet fails at its use (D712.1).

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

use super::{host, item, program};
use crate::std_table::{self, Namespace, Stability};
use crate::Span;

/// The edition whose prelude a program sees.
const EDITION: &str = "2024";

thread_local! {
    /// The std traits the program being resolved names by a path
    /// (`std::ops::Deref`, `fmt::Write` after `use std::fmt;`), by their
    /// last segment, which its bounds and impls carry.
    static WRITTEN: RefCell<BTreeMap<crate::ImmutableString, &'static str>> =
        const { RefCell::new(BTreeMap::new()) };
}

/// Start resolving a program: no std trait is named by a path yet.
pub(crate) fn begin() {
    WRITTEN.with_borrow_mut(BTreeMap::clear);
}

/// The program names the std trait `definition` by a path whose last
/// segment is `name`.
pub(crate) fn written(name: &str, definition: &'static str) {
    WRITTEN.with_borrow_mut(|written| {
        written.entry(name.into()).or_insert(definition);
    });
}

/// The stable std trait a program names `name` by, as rustc's scope has it:
/// through a `use` that binds it (`use std::str::FromStr;`), the prelude
/// (`Iterator`), or a path the program writes (`std::ops::Deref`). Its
/// definition: `core::str::traits::FromStr`. A bare name nothing brings
/// into scope is none (rustc's E0405).
pub(crate) fn named<'a>(
    name: &str,
    uses: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Option<&'static str> {
    for (binding, qualified) in uses {
        if binding == name
            && let Some(definition) = std_table::stable_trait(qualified)
        {
            return Some(definition);
        }
    }
    if let Some(found) = std_table::prelude(EDITION)
        .find(|path| path.kind == "trait" && path.path.rsplit("::").next() == Some(name))
    {
        return (found.stability == Stability::Stable).then_some(found.definition);
    }
    WRITTEN.with_borrow(|written| written.get(name).copied())
}

/// The stable std trait whose last segment is `name`, wherever std declares
/// it: `named`, else the first std module that has one (`std::error::Error`).
/// For what rawr asks of a trait it already knows the program names, never
/// for what the program's scope has.
pub(crate) fn anywhere(name: &str) -> Option<&'static str> {
    named(name, []).or_else(|| {
        std_table::children("std")
            .filter(|module| module.kind == "mod" && module.stability == Stability::Stable)
            .find_map(|module| std_table::stable_trait(&format!("{}::{name}", module.path)))
    })
}

/// The definition of the stable std trait whose last segment is `name`, as
/// a written path names it (`std::fmt::Display`, `fmt::Debug` after `use
/// std::fmt;`).
pub(crate) fn by_path(segments: &[&str]) -> Option<&'static str> {
    let mut path = segments.to_vec();
    if !path.first().is_some_and(|root| std_table::is_root(root)) {
        path.insert(0, "std");
    }
    match std_table::resolve(&path, Namespace::Type) {
        Ok(std_table::Resolved::Path(path))
            if path.kind == "trait" && path.stability == Stability::Stable =>
        {
            if let Some(name) = segments.last() {
                written(name, path.definition);
            }
            Some(path.definition)
        }
        _ => None,
    }
}

/// A std trait's definition and the provided functions its declaration
/// carries.
type Key = (&'static str, Vec<&'static str>);

thread_local! {
    /// Each std trait's declaration, by its definition and the provided
    /// functions it carries, resolved once per thread.
    static DECLARED: RefCell<HashMap<Key, Option<item::Trait>>> =
        RefCell::new(HashMap::new());
}

/// The declaration of the stable std trait `definition`, resolved as a
/// program's own trait is: its generics with their defaults, its
/// supertraits, its associated types with their bounds, its required
/// functions, and the provided functions named in `overridden`, which an
/// impl of the program overrides (`Iterator::size_hint`, `Error::source`).
///
/// A function whose signature names a type rawr has no representation for
/// yet (`io::Write::write`'s `io::Error`) is left out, so an impl declares
/// fine and its own signature says so.
pub(crate) fn declaration(
    definition: &str,
    overridden: &[&str],
    host: &host::Registry,
) -> Option<item::Trait> {
    let definition = std_table::item(definition)?.definition;
    let mut provided = std_table::members(definition)
        .filter(|member| {
            member.kind == "assoc-fn"
                && member.provided()
                && member.stability == Stability::Stable
                && overridden.contains(&member.name())
        })
        .map(|member| member.name())
        .collect::<Vec<_>>();
    provided.sort_unstable();
    let key = (definition, provided);
    if let Some(found) = DECLARED.with_borrow(|declared| declared.get(&key).cloned()) {
        return found;
    }
    let declared = resolve(definition, &key.1, host);
    DECLARED.with_borrow_mut(|cache| cache.insert(key, declared.clone()));
    declared
}

/// The std traits rawr reads without their `Rhs = Self` parameter, which
/// its runtime compares by (`RUNS`): a binding until #708.
const SELF_ONLY: &[&str] = &["core::cmp::PartialEq", "core::cmp::PartialOrd"];

fn resolve(
    definition: &'static str,
    provided: &[&str],
    host: &host::Registry,
) -> Option<item::Trait> {
    let mut source = std_table::trait_source(definition, provided)?;
    // rawr compares a value with one of its own type (#708, D711 step 3):
    // `PartialEq` and `PartialOrd` are read with their `Rhs` as `Self`.
    if SELF_ONLY.contains(&definition) {
        source = source
            .replace("<Rhs = Self>", "")
            .replace("<Rhs>", "")
            .replace("Rhs", "Self");
    }
    let mut parsed = crate::parser::parse_trait(&source)?;
    let attempt = |parsed: crate::item::Trait| {
        super::resolve_item(
            crate::item::Id::from_index(0),
            program::source::Id::ROOT,
            None,
            None,
            crate::item::Parsed::Trait(parsed),
            &BTreeMap::new(),
            host,
            #[cfg(feature = "rust-import")]
            None,
        )
        .ok()
    };
    let shell = match attempt(parsed.clone()) {
        Some(shell) => shell,
        None => {
            // Each function that resolves on its own.
            let kept = parsed
                .functions
                .iter()
                .zip(parsed.provided.iter())
                .filter(|(function, provided)| {
                    let mut one = parsed.clone();
                    one.functions = Box::new([(*function).clone()]);
                    one.provided = Box::new([**provided]);
                    attempt(one).is_some()
                })
                .map(|(function, provided)| (function.clone(), *provided))
                .collect::<Vec<_>>();
            parsed.provided = kept.iter().map(|(_, provided)| *provided).collect();
            parsed.functions = kept.into_iter().map(|(function, _)| function).collect();
            attempt(parsed)?
        }
    };
    let item::Kind::Trait(mut declared) = shell.kind else {
        return None;
    };
    // `Self::Output` of `Add` is `<Self as Add>::Output`, as std means it:
    // a bound by several traits names each one's own.
    let own = declared
        .associated
        .iter()
        .map(|name| name.text.clone())
        .collect::<Vec<_>>();
    let trait_name = declared.name.text.clone();
    let mut functions = declared.functions.to_vec();
    for function in &mut functions {
        for parameter in function.parameters.iter_mut() {
            qualify(&mut parameter.ty, &trait_name, &own);
        }
        if let Some(output) = &mut function.output {
            qualify(output, &trait_name, &own);
        }
    }
    declared.functions = functions.into_boxed_slice();
    // std's: no span of the program's.
    declared.name.span = Span::NONE;
    declared.std = Some(definition);
    Some(declared)
}

/// `Self::Name` in `ty` as `<Self as trait_name>::Name` for each of the
/// trait's own associated types `own`.
fn qualify(
    ty: &mut super::type_ref::Type,
    trait_name: &crate::ImmutableString,
    own: &[crate::ImmutableString],
) {
    use super::type_ref::{Argument, Target, Type};
    match ty {
        Type::Unit(_) => {}
        Type::Tuple { members, .. } => {
            for member in members.iter_mut() {
                qualify(member, trait_name, own);
            }
        }
        Type::Reference { target, .. } => qualify(target, trait_name, own),
        Type::Path(path) => {
            if let Target::Projection(base, written @ None) = &mut path.target
                && matches!(&**base, Type::Path(base)
                    if base.target == Target::Parameter && base.name == "Self")
                && own.contains(&path.name)
            {
                *written = Some(trait_name.clone());
            }
            for argument in path.arguments.iter_mut() {
                if let Argument::Type(ty) = argument {
                    qualify(ty, trait_name, own);
                }
            }
        }
    }
}

/// The program's `use` bindings, by name: what a trait's name resolves
/// through.
pub(crate) fn uses(items: &[item::Shell]) -> Vec<(crate::ImmutableString, crate::ImmutableString)> {
    items
        .iter()
        .filter_map(|shell| match &shell.kind {
            item::Kind::Use(used) => Some(used.bindings.iter()),
            _ => None,
        })
        .flatten()
        .map(|binding| (binding.name.text.clone(), binding.qualified.clone()))
        .collect()
}

/// The std traits the program's items name, which it doesn't declare
/// itself, with their supertraits: each by its name, with its declaration.
/// An impl's functions that override a provided function come with it.
pub(crate) fn declarations(
    items: &[item::Shell],
    host: &host::Registry,
) -> BTreeMap<crate::ImmutableString, item::Trait> {
    let declared = items
        .iter()
        .filter_map(|shell| match &shell.kind {
            item::Kind::Trait(own) => Some(own.name.text.clone()),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    let mut named = Vec::<crate::ImmutableString>::new();
    let mut overridden = BTreeMap::<crate::ImmutableString, Vec<crate::ImmutableString>>::new();
    let generics = |generics: &[item::Generic], named: &mut Vec<crate::ImmutableString>| {
        for generic in generics {
            named.extend(generic.traits.iter().map(|name| name.text.clone()));
        }
    };
    for shell in items {
        match &shell.kind {
            item::Kind::Struct(own) => generics(&own.generics, &mut named),
            item::Kind::Enum(own) => generics(&own.generics, &mut named),
            item::Kind::Function(own) => generics(&own.generics, &mut named),
            item::Kind::Impl(own) => {
                generics(&own.generics, &mut named);
                for function in own.functions.iter() {
                    generics(&function.generics, &mut named);
                }
                if let Some(item::TraitImpl::User { name, .. }) = &own.trait_ {
                    named.push(name.text.clone());
                    overridden.entry(name.text.clone()).or_default().extend(
                        own.functions
                            .iter()
                            .map(|function| function.name.text.clone()),
                    );
                }
            }
            item::Kind::Trait(own) => {
                for function in own.functions.iter() {
                    generics(&function.generics, &mut named);
                }
                named.extend(own.supertraits.1.iter().map(|name| name.text.clone()));
                for (_, traits) in own.associated_bounds.iter() {
                    named.extend(traits.iter().map(|name| name.text.clone()));
                }
            }
            item::Kind::Use(_) => {}
        }
    }
    let uses = uses(items);
    let mut found = BTreeMap::new();
    while let Some(name) = named.pop() {
        if declared.contains(&name) || found.contains_key(&name) {
            continue;
        }
        let Some(definition) = self::named(
            &name,
            uses.iter()
                .map(|(binding, qualified)| (binding.as_str(), qualified.as_str())),
        ) else {
            continue;
        };
        let names = overridden
            .get(&name)
            .map(|names| names.iter().map(|name| name.as_str()).collect::<Vec<_>>())
            .unwrap_or_default();
        let Some(trait_) = declaration(definition, &names, host) else {
            continue;
        };
        named.extend(trait_.supertraits.1.iter().map(|name| name.text.clone()));
        found.insert(name, trait_);
    }
    found
}

/// Whether the std trait `definition` has a stable function `name`, as
/// `Iterator::size_hint`.
pub(crate) fn member(definition: &str, name: &str) -> bool {
    std_table::item(&format!("{definition}::{name}"))
        .is_some_and(|member| member.kind == "assoc-fn" && member.stability == Stability::Stable)
}

/// The std traits whose methods rawr's runtime runs for its own values,
/// which a trait's dispatcher falls back to for a value with no impl of the
/// program's (D711's execution mapping): the derivable traits, formatting,
/// iteration and collection, indexing, `Error`, `Drop`, and the operator
/// traits of `core::ops::arith` and `core::ops::bit`.
const RUNS: &[&str] = &[
    "core::clone::Clone",
    "core::marker::Copy",
    "core::cmp::PartialEq",
    "core::cmp::Eq",
    "core::cmp::PartialOrd",
    "core::cmp::Ord",
    "core::hash::Hash",
    "core::default::Default",
    "core::fmt::Debug",
    "core::fmt::Display",
    "core::iter::traits::iterator::Iterator",
    "core::iter::traits::collect::FromIterator",
    "core::ops::index::Index",
    "core::ops::index::IndexMut",
    "core::error::Error",
    "core::ops::drop::Drop",
];

/// Whether `name` is one of std's operator traits (`Add`, `BitOrAssign`),
/// the traits of `core::ops::arith` and `core::ops::bit`.
pub(crate) fn is_operator(name: &str) -> bool {
    anywhere(name).is_some_and(|definition| {
        definition.starts_with("core::ops::arith::") || definition.starts_with("core::ops::bit::")
    })
}

/// Whether rawr's runtime runs the std trait `definition`'s methods for
/// its own values (`RUNS`).
pub(crate) fn runs_builtin(definition: &str) -> bool {
    RUNS.contains(&definition)
        || definition.starts_with("core::ops::arith::")
        || definition.starts_with("core::ops::bit::")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What an impl is checked against: each function's receiver, inputs
    /// and output, the associated types, the generics with their defaults
    /// and the supertraits.
    fn summary(declared: &item::Trait) -> String {
        let functions = declared
            .functions
            .iter()
            .zip(declared.provided.iter())
            .map(|(function, provided)| {
                let receiver = match &function.receiver {
                    None => "",
                    Some(crate::item::Receiver::Shared(_)) => "&self ",
                    Some(crate::item::Receiver::Mutable(_)) => "&mut self ",
                    Some(_) => "self ",
                };
                format!(
                    "{}{}({receiver}{}) -> {}{}",
                    function.name.text,
                    if function.generics.is_empty() {
                        ""
                    } else {
                        "<..>"
                    },
                    function
                        .parameters
                        .iter()
                        .map(|parameter| super::super::canonical_type_name(&parameter.ty))
                        .collect::<Vec<_>>()
                        .join(", "),
                    function
                        .output
                        .as_ref()
                        .map_or_else(|| "()".to_owned(), super::super::canonical_type_name),
                    if *provided { " provided" } else { "" }
                )
            })
            .collect::<Vec<_>>();
        let generics = declared
            .generics
            .iter()
            .map(|generic| match &generic.default {
                Some(default) => format!(
                    "{}={}",
                    generic.name.text,
                    super::super::canonical_type_name(default)
                ),
                None => generic.name.text.to_string(),
            })
            .collect::<Vec<_>>();
        let associated = declared
            .associated
            .iter()
            .map(|name| name.text.to_string())
            .collect::<Vec<_>>();
        let supertraits = declared
            .supertraits
            .1
            .iter()
            .map(|name| name.text.to_string())
            .collect::<Vec<_>>();
        format!(
            "<{}> : {:?} {} | {} | {}",
            generics.join(", "),
            declared.supertraits.0,
            supertraits.join(" + "),
            associated.join(", "),
            functions.join("; ")
        )
    }

    fn declared(name: &str) -> String {
        let host = host::Registry::new();
        let definition = anywhere(name).unwrap_or_else(|| panic!("std has `{name}`"));
        summary(&declaration(definition, &[], &host).unwrap_or_else(|| panic!("`{name}` declares")))
    }

    /// std's declarations, as an impl is checked against them. Shadow mode
    /// (D711.3) compared these with the hand list they replace; they differ
    /// where std's is what rustc checks: `Hash` requires `hash`, `Copy`
    /// needs `Clone`, `Eq` and `PartialOrd` need `PartialEq`, `Error` needs
    /// `Debug` and `Display`, `IndexMut`'s `Output` is its `Index`'s, and
    /// the pinned nightly gives `Drop::drop` a default.
    #[test]
    fn std_trait_declarations_are_stds() {
        assert_eq!(
            declared("FromStr"),
            "<> : Set(0)  | Err | from_str(&str) -> Result<Self, Self::Err>"
        );
        assert_eq!(
            declared("Iterator"),
            "<> : Set(0)  | Item | next(&mut self ) -> Option<Self::Item>"
        );
        assert_eq!(
            declared("Add"),
            "<Rhs=Self> : Set(0)  | Output | add(self Rhs) -> Self::Output"
        );
        assert_eq!(
            declared("Display"),
            "<> : Set(0)  |  | fmt(&self &mut String) -> Result<(), ()>"
        );
        assert_eq!(
            declared("PartialOrd"),
            "<> : Set(16)  |  | partial_cmp(&self &Self) -> Option<Ordering>"
        );
        assert_eq!(
            declared("IndexMut"),
            "<Idx> : Set(0) Index |  | index_mut(&mut self Idx) -> &mut Self::Output"
        );
        assert_eq!(declared("Error"), "<> : Set(2) Display |  | ");
        // `io::Write::write` names `io::Error`, which rawr has no
        // representation for yet: the impl's own signature says so.
        let host = host::Registry::new();
        let write = declaration("core::io::write::Write", &[], &host).expect("io::Write declares");
        assert!(write.functions.is_empty(), "{}", summary(&write));
        assert_eq!(anywhere("Write"), Some("core::fmt::Write"));
        // A bare name nothing brings into scope is none (E0405).
        begin();
        assert_eq!(named("Deref", []), None);
        assert_eq!(
            by_path(&["std", "ops", "Deref"]),
            Some("core::ops::deref::Deref")
        );
        assert_eq!(named("Deref", []), Some("core::ops::deref::Deref"));
        begin();
    }
}
