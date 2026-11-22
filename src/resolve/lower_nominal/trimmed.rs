//! How a message names a std type, as rustc's trimmed paths do: by its
//! name alone where no other type the program sees has that name, else by
//! its std path. rustc writes `Vec`, `Option` and `RangeTo` bare, but
//! `std::ops::Range`, since core also has `core::range::Range`, and
//! `std::rc::Weak`, beside `std::sync::Weak`.

use super::models::Models;
use crate::resolve::Program;
use crate::std_table;
use crate::ImmutableString;
use std::borrow::Cow;
use std::collections::BTreeSet;

/// Whether rustc writes a std type named `name` by its name alone: no
/// other type of std's crates has the name, nor any type or trait of the
/// program, nor any name the program reads through a glob import of a std
/// module (`glob_names`).
pub(super) fn unique(name: &str, models: &Models) -> bool {
    std_table::sole_type(name).is_some()
        && !models
            .by_definition
            .values()
            .any(|nominal| nominal.name() == name)
        && !models.traits.contains_key(name)
        && !models.glob_names.contains(name)
}

/// How a message names the std type defined at `definition`
/// (`core::ops::range::Range`): by its name alone where it is `unique`,
/// else by its shortest public std path, `std::ops::Range`.
pub(super) fn std_name(definition: &'static str, models: &Models) -> Cow<'static, str> {
    let name = definition.rsplit("::").next().unwrap_or(definition);
    if unique(name, models) {
        return Cow::Borrowed(name);
    }
    Cow::Borrowed(public_path(definition).unwrap_or(name))
}

/// The type at the facade path `path`, when it is one of Rawr's std's, as
/// rustc names std's own: by its name alone where it is `unique`, else by
/// its std path, as `std::fmt::Alignment` is beside `std::mem::Alignment`,
/// and `std::path::PathBuf` beside the program's own `PathBuf`. `None` for
/// any other type. A type std's alias names is named as the type it
/// names, as rustc names an `AtomicUsize` `Atomic<usize>`.
pub(super) fn std_type_name(path: &str, models: &Models) -> Option<String> {
    let rest = path
        .strip_prefix(crate::std_facade::ROOT)?
        .strip_prefix("::")?;
    let std = format!("std::{rest}");
    let (std, arguments) = match std_table::aliased(&std) {
        Some(target) => target.split_at(target.find('<').unwrap_or(target.len())),
        None => (std.as_str(), ""),
    };
    let name = std.rsplit("::").next().unwrap_or(std);
    Some(if unique(name, models) {
        format!("{name}{arguments}")
    } else {
        format!("{std}{arguments}")
    })
}

/// The std type defined at `definition` as rustc's E0599 names it where no
/// method of the name is found on any of the type's instances: with its
/// definition's parameters, defaults and all, `std::ops::Range<Idx>`,
/// `HashMap<K, V, S, A>`, `std::slice::Iter<'a, T>`.
pub(super) fn declared(definition: &'static str, models: &Models) -> String {
    let name = std_name(definition, models);
    let Some(item) = std_table::item(definition) else {
        return name.into_owned();
    };
    let parameters: Vec<String> = item
        .generics()
        .map(|parameter| match parameter.kind {
            std_table::ParameterKind::Lifetime => format!("'{}", parameter.name),
            _ => parameter.name.to_owned(),
        })
        .collect();
    if parameters.is_empty() {
        return name.into_owned();
    }
    format!("{name}<{}>", parameters.join(", "))
}

/// The std type defined at `definition` as rustc names a value of it whose
/// parameters are all lifetimes, each elided: `Chars<'_>`. `None` for one
/// with a type or const parameter, whose arguments a value has.
pub(super) fn elided(definition: &'static str, models: &Models) -> Option<String> {
    let item = std_table::item(definition)?;
    let mut lifetimes = 0;
    for parameter in item.generics() {
        if parameter.kind != std_table::ParameterKind::Lifetime {
            return None;
        }
        lifetimes += 1;
    }
    let name = std_name(definition, models);
    if lifetimes == 0 {
        return Some(name.into_owned());
    }
    Some(format!("{name}<{}>", vec!["'_"; lifetimes].join(", ")))
}

/// The shortest stable public std path of `definition`, read once per
/// definition: rustc's visible path of a std type.
fn public_path(definition: &'static str) -> Option<&'static str> {
    use std::collections::HashMap;
    thread_local! {
        static PATHS: std::cell::RefCell<HashMap<&'static str, Option<&'static str>>> =
            std::cell::RefCell::new(HashMap::new());
    }
    PATHS.with(|paths| {
        *paths
            .borrow_mut()
            .entry(definition)
            .or_insert_with(|| std_table::public_path(definition))
    })
}

/// The names of std's modules a source's glob imports bring in that the
/// source writes, which rustc never writes a std type by alone (its
/// `glob_map`): `RangeTo`, where a program with `use std::ops::*;` writes
/// `RangeTo`. A name the source never writes, as `RangeTo` of `..1`, is
/// read through no import.
pub(super) fn glob_names(program: &Program) -> BTreeSet<ImmutableString> {
    let mut names = BTreeSet::new();
    for (source, imports) in &program.lexical {
        let globbed = imports
            .iter()
            .filter_map(|used| match &used.tree {
                crate::item::UseTree::Glob { prefix, .. } => {
                    let segments: Vec<&str> = prefix
                        .segments
                        .iter()
                        .map(|segment| segment.name.text.as_str())
                        .collect();
                    crate::resolve::std_path::glob(&segments.join("::"))
                }
                _ => None,
            })
            .flatten()
            .collect::<Vec<_>>();
        if globbed.is_empty() {
            continue;
        }
        let written: BTreeSet<&str> = program
            .source
            .text_at(*source)
            .split(|character: char| !(character.is_alphanumeric() || character == '_'))
            .collect();
        names.extend(
            globbed
                .into_iter()
                .filter(|name| written.contains(name.as_str())),
        );
    }
    names
}
