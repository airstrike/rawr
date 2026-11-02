//! D711 step 1's shadow mode: the std table and rawr's hand lists both
//! answer, and each test fails on any disagreement it doesn't name. The
//! named ones are the hand lists' bugs the table fixes.

use super::std_path;
use crate::std_table;
use crate::{ImmutableString, Span};
use std::collections::BTreeSet;

/// Every `root::rest` for each path std has under any root: the hand list
/// accepts some under the wrong root.
fn candidates() -> BTreeSet<String> {
    let mut rests = BTreeSet::new();
    for root in ["std", "core", "alloc"] {
        let mut stack = vec![root.to_owned()];
        while let Some(module) = stack.pop() {
            for child in std_table::children(&module) {
                let rest = &child.path[root.len() + 2..];
                if rests.insert(rest.to_owned())
                    && child.kind == "mod"
                    && rest.split("::").count() < 3
                {
                    stack.push(child.path.to_owned());
                }
            }
        }
    }
    let mut out = BTreeSet::new();
    for rest in rests {
        for root in ["std", "core", "alloc"] {
            out.insert(format!("{root}::{rest}"));
        }
    }
    out
}

fn table_import(path: &str) -> bool {
    matches!(std_path::import(path, Span::NONE), Ok(Some(_)))
}

#[test]
fn std_shadow_imports() {
    let mut hand_only = Vec::new();
    for path in candidates() {
        if super::std_fmt_import(&path) && !table_import(&path) {
            hand_only.push(path);
        }
    }
    // rustc refuses each: core has no `collections`, `thread`, `rc` or
    // `string`, and `Instant` and `Arc` aren't core's.
    assert_eq!(
        hand_only,
        [
            "core::collections",
            "core::collections::BTreeMap",
            "core::collections::BTreeSet",
            "core::collections::HashMap",
            "core::collections::HashSet",
            "core::collections::VecDeque",
            "core::rc",
            "core::rc::Rc",
            "core::string::FromUtf8Error",
            "core::sync::Arc",
            "core::thread",
            "core::thread::sleep",
            "core::time::Instant",
        ]
    );
}

#[test]
fn std_shadow_globs() {
    let mut disagreements = Vec::new();
    for root in ["std", "core"] {
        for module in [
            "fmt",
            "cmp",
            "collections",
            "hash",
            "convert",
            "str",
            "mem",
            "time",
            "thread",
            "iter",
            "marker",
            "cell",
            "error",
            "rc",
            "num",
        ] {
            let module = format!("{root}::{module}");
            let hand = super::std_glob(&module);
            let table = std_path::glob(&module).unwrap_or_default();
            for name in hand {
                if !table.contains(&name) {
                    disagreements.push(format!("{module}::{name}"));
                }
            }
        }
    }
    assert_eq!(
        disagreements,
        [
            "core::collections::HashMap",
            "core::collections::HashSet",
            "core::collections::BTreeMap",
            "core::collections::BTreeSet",
            "core::collections::VecDeque",
            "core::time::Instant",
        ]
    );
}

fn segments(path: &str) -> Vec<crate::item::type_ref::Segment> {
    path.split("::")
        .map(|name| crate::item::type_ref::Segment {
            name: crate::item::Name {
                text: ImmutableString::from(name),
                span: Span::NONE,
            },
            arguments: Box::default(),
            span: Span::NONE,
        })
        .collect()
}

#[test]
fn std_shadow_trait_paths() {
    let modules = [
        "fmt", "clone", "cmp", "default", "marker", "iter", "ops", "hash", "error",
    ];
    let mut hand_only = BTreeSet::new();
    let mut table_only = BTreeSet::new();
    for module in modules {
        for name in super::BUILTIN_TRAITS {
            for path in [
                format!("{module}::{name}"),
                format!("std::{module}::{name}"),
                format!("core::{module}::{name}"),
            ] {
                let path_segments = segments(&path);
                let hand = super::std_trait(&path_segments).is_some();
                let table = std_path::trait_path(&path_segments).is_some();
                if hand && !table {
                    hand_only.insert(path);
                } else if table && !hand {
                    table_only.insert(path);
                }
            }
        }
    }
    // The hand list takes every trait under every module it knows, as
    // `std::ops::Display`; std has each in one module.
    for path in &hand_only {
        let path = if path.starts_with("std::") || path.starts_with("core::") {
            path.clone()
        } else {
            format!("std::{path}")
        };
        assert!(
            std_table::path(&path, std_table::Namespace::Type)
                .is_none_or(|found| found.kind != "trait"),
            "the hand list and std agree on `{path}`, yet the table refuses it"
        );
    }
    assert!(table_only.is_empty(), "{table_only:?}");
    assert!(!hand_only.contains("std::fmt::Display"));
    assert!(hand_only.contains("std::ops::Display"));
}

#[test]
fn std_shadow_arities() {
    let mut disagreements = Vec::new();
    for name in [
        "Option",
        "Vec",
        "Box",
        "Result",
        "HashMap",
        "BTreeMap",
        "HashSet",
        "BTreeSet",
        "Reverse",
        "RefCell",
        "Cell",
        "Rc",
        "Arc",
        "RangeInclusive",
    ] {
        let hand = super::type_ref::hand_arity(name).expect("a hand arity");
        let table = std_path::arity(name).expect("a std type");
        if table != (hand, hand) {
            disagreements.push((name, hand, table));
        }
    }
    // std's `S = RandomState`, which rawr refused.
    assert_eq!(
        disagreements,
        [("HashMap", 2, (2, 3)), ("HashSet", 1, (1, 2))]
    );
}
