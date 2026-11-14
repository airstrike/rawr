//! std's types in a crate's signature (D711 step 5): which of rawr's
//! representations each std definition is. A definition is named by its
//! definition path, the key std's own table (`src/std_table/items.tsv`)
//! and rawr's binding of its native types (`resolve::lower_nominal::
//! std_impl`) use. The binding states no signature, trait or arity: a
//! type's arguments are read from the signature that names it.

use crate::api::model;

/// The rawr representation a std type is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Std {
    /// alloc's `String`: rawr's string.
    String,
    /// core's `fmt::Arguments<'a>`, as an input: rawr's string of the text
    /// `format_args!` formats, formatted into the call.
    Arguments,
    /// core's `PhantomData<T>`, which holds nothing: rawr's unit.
    PhantomData,
    /// core's `Option<T>`: rawr's `Option`.
    Option,
    /// core's `Result<T, E>`: rawr's `Result`.
    Result,
    /// alloc's `Vec<T>`: rawr's array.
    Vec,
    /// core's `Range<T>`: rawr's range value.
    Range,
    /// core's `RangeInclusive<T>`: rawr's inclusive range.
    RangeInclusive,
    /// alloc's `Cow<'a, B>`: what it holds, copied out.
    Cow,
    /// alloc's `Box<T>`: its value, which rawr holds as itself.
    Box,
    /// alloc's `Rc<T>` (`arc` false) or `Arc<T>` of a crate's `T`: rawr's
    /// counted pointer, whose value is the crate's own pointer (#775).
    /// Of any other `T` it is [`Unrun::Rc`] or [`Unrun::Arc`].
    Counted { arc: bool },
    /// std's `HashMap`, `HashSet`, `BTreeMap` or `BTreeSet`, by its name:
    /// rawr's collection of that kind, whose keys are rawr's strings,
    /// numbers, chars or bools (#777). Of other keys it is not run yet.
    Keyed(&'static str, Unrun),
    /// core's `Duration`: rawr's own.
    Duration,
    /// std's `Instant`: rawr's own.
    Instant,
    /// std's `thread::JoinHandle<T>`: rawr's thread handle, of any `T` a
    /// crate gives and of a `T` that crosses by value a crate takes (#1266,
    /// #1274, #1275). Elsewhere it is [`Unrun::JoinHandle`].
    JoinHandle,
    /// A std type rawr's std holds as its Rust value (`crossing::Std`), by
    /// its path under `std`, and whether it is `Clone`.
    Held { path: &'static str, clone: bool },
    /// An unsized borrowed form of a [`Std::Held`] type, by the held type's
    /// path: rawr's std names `Path` as `PathBuf`, which a borrow lends.
    Lent { owned: &'static str },
    /// A std type rawr represents that no crate's signature takes or gives
    /// yet: each use of it is "rawr doesn't run `X` yet" (D712.1).
    Unrun(Unrun),
}

/// A std type rawr represents but doesn't take from or give to a crate
/// yet, by its public path.
#[derive(
    Clone, Copy, Debug, serde::Deserialize, Eq, Ord, PartialEq, PartialOrd, serde::Serialize,
)]
pub enum Unrun {
    /// `std::rc::Rc` of other than a crate's type, as `Rc<str>` or
    /// `Rc<i64>`: rawr's counted pointer holds that value itself, which no
    /// crate's pointer can share without a copy.
    Rc,
    /// `std::sync::Arc`, as `Rc` is.
    Arc,
    /// `std::collections::HashMap` of keys other than rawr's strings,
    /// numbers, chars or bools: rawr orders its keys where it holds them,
    /// and a crate's key type has no order rawr reads yet.
    HashMap,
    /// `std::collections::HashSet`, as `HashMap` is.
    HashSet,
    /// `std::collections::BTreeMap`, as `HashMap` is.
    BTreeMap,
    /// `std::collections::BTreeSet`, as `HashMap` is.
    BTreeSet,
    /// `std::thread::JoinHandle` the crate lends or a field holds, or one
    /// the crate takes of a `T` that holds a crate value, which a program's
    /// thread gives on no call's arena.
    JoinHandle,
    /// Any other std type: one rawr has no representation for yet.
    Other,
}

impl Unrun {
    /// Why an item naming this type can't be used yet, as
    /// `reject::Kind::describe` says it.
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            Self::Rc => "rawr doesn't run `std::rc::Rc` in a crate's signature yet",
            Self::Arc => "rawr doesn't run `std::sync::Arc` in a crate's signature yet",
            Self::HashMap => {
                "rawr doesn't run `std::collections::HashMap` in a crate's signature yet"
            }
            Self::HashSet => {
                "rawr doesn't run `std::collections::HashSet` in a crate's signature yet"
            }
            Self::BTreeMap => {
                "rawr doesn't run `std::collections::BTreeMap` in a crate's signature yet"
            }
            Self::BTreeSet => {
                "rawr doesn't run `std::collections::BTreeSet` in a crate's signature yet"
            }
            Self::JoinHandle => {
                "rawr doesn't run `std::thread::JoinHandle` here in a crate's signature yet"
            }
            Self::Other => "rawr doesn't run this standard library type in a crate's signature yet",
        }
    }
}

/// Each std definition rawr represents, by its definition path.
pub(crate) const BINDING: [(&[&str], Std); 26] = [
    (&["alloc", "borrow", "Cow"], Std::Cow),
    (&["alloc", "boxed", "Box"], Std::Box),
    (
        &["alloc", "collections", "btree", "map", "BTreeMap"],
        Std::Keyed("BTreeMap", Unrun::BTreeMap),
    ),
    (
        &["alloc", "collections", "btree", "set", "BTreeSet"],
        Std::Keyed("BTreeSet", Unrun::BTreeSet),
    ),
    (&["alloc", "rc", "Rc"], Std::Counted { arc: false }),
    (&["alloc", "string", "String"], Std::String),
    (&["alloc", "sync", "Arc"], Std::Counted { arc: true }),
    (&["alloc", "vec", "Vec"], Std::Vec),
    (&["core", "fmt", "Arguments"], Std::Arguments),
    (
        &["core", "io", "error", "Error"],
        Std::Held {
            path: "io::Error",
            clone: false,
        },
    ),
    (
        &["core", "io", "error", "ErrorKind"],
        Std::Held {
            path: "io::ErrorKind",
            clone: true,
        },
    ),
    (&["core", "marker", "PhantomData"], Std::PhantomData),
    (&["core", "ops", "range", "Range"], Std::Range),
    (
        &["core", "ops", "range", "RangeInclusive"],
        Std::RangeInclusive,
    ),
    (&["core", "option", "Option"], Std::Option),
    (
        &["core", "panic", "location", "Location"],
        Std::Held {
            path: "panic::Location",
            clone: true,
        },
    ),
    (&["core", "result", "Result"], Std::Result),
    (&["core", "time", "Duration"], Std::Duration),
    (
        &["std", "collections", "hash", "map", "HashMap"],
        Std::Keyed("HashMap", Unrun::HashMap),
    ),
    (
        &["std", "collections", "hash", "set", "HashSet"],
        Std::Keyed("HashSet", Unrun::HashSet),
    ),
    (
        &["std", "ffi", "os_str", "OsStr"],
        Std::Lent {
            owned: "ffi::OsString",
        },
    ),
    (
        &["std", "ffi", "os_str", "OsString"],
        Std::Held {
            path: "ffi::OsString",
            clone: true,
        },
    ),
    (
        &["std", "path", "Path"],
        Std::Lent {
            owned: "path::PathBuf",
        },
    ),
    (
        &["std", "path", "PathBuf"],
        Std::Held {
            path: "path::PathBuf",
            clone: true,
        },
    ),
    (
        &["std", "thread", "join_handle", "JoinHandle"],
        Std::JoinHandle,
    ),
    (&["std", "time", "Instant"], Std::Instant),
];

/// The representation of the std type `definition` is, or `None` for a
/// definition outside the sysroot. A sysroot type the binding doesn't
/// list is [`Unrun::Other`].
pub(crate) fn of(definition: &model::Definition) -> Option<Std> {
    let unit = definition.id().unit();
    unit.authority().sysroot()?;
    if !matches!(unit.crate_name(), "core" | "alloc" | "std") {
        return Some(Std::Unrun(Unrun::Other));
    }
    let last = definition.display_path().last()?;
    Some(
        BINDING
            .iter()
            .find(|(path, _)| {
                path.last() == Some(&last) && definition.display_path().eq(path.iter().copied())
            })
            .map_or(Std::Unrun(Unrun::Other), |(_, std)| *std),
    )
}

/// The `T` of std's `sync::LazyLock<T, F>`, the value a `static` of one
/// derefs to (#610). rustc's own path to it goes through a private module
/// (`std::sync::lazy_lock::LazyLock`), so only its crate, its `sync`
/// module and its name are read.
pub(crate) fn lazy_lock_target<'a>(
    definition: &model::Definition,
    arguments: &'a [model::Argument],
) -> Option<&'a model::Type> {
    let unit = definition.id().unit();
    unit.authority().sysroot()?;
    let mut path = definition.display_path();
    let lazy = unit.crate_name() == "std"
        && path.next() == Some("std")
        && path.next() == Some("sync")
        && definition.display_path().last() == Some("LazyLock");
    lazy.then(|| type_argument(arguments, 0)).flatten()
}

/// The `index`th type argument of a std type, its lifetimes skipped:
/// `T` of `Vec<T, A>`, `B` of `Cow<'a, B>`.
pub(crate) fn type_argument(arguments: &[model::Argument], index: usize) -> Option<&model::Type> {
    arguments
        .iter()
        .filter_map(|argument| match argument {
            model::Argument::Type(ty) => Some(ty),
            _ => None,
        })
        .nth(index)
}

/// The `index`th type argument of `definition` with `arguments` when it is
/// the std type `std`.
pub(crate) fn argument_of<'a>(
    definition: &model::Definition,
    arguments: &'a [model::Argument],
    std: Std,
    index: usize,
) -> Option<&'a model::Type> {
    (of(definition) == Some(std))
        .then(|| type_argument(arguments, index))
        .flatten()
}

/// The held std types, by their paths under `std`, and whether each is
/// `Clone`: what `identity::Definition::std` names.
pub(crate) fn held() -> impl Iterator<Item = (&'static str, bool)> {
    BINDING.iter().filter_map(|(_, std)| match std {
        Std::Held { path, clone } => Some((*path, *clone)),
        _ => None,
    })
}

/// The held type a std definition at `path` is, itself or as its borrowed
/// form, by its path under `std`.
pub(crate) fn held_path(path: &[&str]) -> Option<&'static str> {
    BINDING.iter().find_map(|(key, std)| match std {
        Std::Held { path: held, .. } if *key == path => Some(*held),
        Std::Lent { owned } if *key == path => Some(*owned),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::{Std, BINDING};

    const ITEMS: &str = include_str!("../../../src/std_table/items.tsv");
    const IMPLS: &str = include_str!("../../../src/std_table/impls.tsv");

    /// The kind std's table gives the definition at `path`.
    fn kind(path: &str) -> Option<&'static str> {
        ITEMS.lines().find_map(|line| {
            let mut fields = line.split('\t');
            (fields.next() == Some(path))
                .then(|| fields.next())
                .flatten()
        })
    }

    /// Every type the binding names is a struct or enum in std's table, as
    /// the pinned toolchain defines it, and a held type's `Clone` is std's
    /// own impl.
    #[test]
    fn the_binding_names_std_tables_types() {
        for (path, std) in BINDING {
            let joined = path.join("::");
            assert!(
                matches!(kind(&joined), Some("struct" | "enum")),
                "{joined} is no type of std's table"
            );
            if let Std::Held { clone, .. } = std {
                let cloned = IMPLS.lines().any(|line| {
                    let mut fields = line.split('\t');
                    fields.next() == Some("core::clone::Clone") && fields.next() == Some(&joined)
                });
                assert_eq!(cloned, clone, "{joined}'s Clone");
            }
        }
        let mut sorted = BINDING.map(|(path, _)| path.join("::"));
        sorted.sort();
        assert_eq!(
            sorted.to_vec(),
            BINDING.map(|(path, _)| path.join("::")).to_vec()
        );
    }
}
