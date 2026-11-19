//! What native modules below a marked module see of its private items.
//! Rust lets a module see its ancestors' private items, so a native
//! module reaches them by the paths it writes (`super::scale`,
//! `crate::Reading`). The shadow crate stands a trampoline or a copy for
//! each item seen that way, and for nothing else private, so a private
//! item no native code names stays the interpreter's own and its edits
//! swap.

use std::collections::{BTreeMap, BTreeSet};

use crate::item;
use crate::ImmutableString;

/// The private items of one marked module that native modules below it
/// name, by the paths their sources write.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Seen {
    /// The items these names name, and nothing else private.
    Names(BTreeSet<String>),
    /// Every item: a path names the module itself, as `use super::*;`
    /// does, so whatever is in it may be reached.
    Every,
}

/// Each marked module's [`Seen`], by its module path. A module no native
/// module below names anything of is missing.
pub(crate) type Below = BTreeMap<Box<[ImmutableString]>, Seen>;

impl Default for Seen {
    /// Nothing private is seen.
    fn default() -> Self {
        Self::Names(BTreeSet::new())
    }
}

impl Seen {
    /// Whether a native module below sees the item `name`.
    pub(crate) fn contains(&self, name: &str) -> bool {
        match self {
            Self::Names(names) => names.contains(name),
            Self::Every => true,
        }
    }

    /// A native module below names the item `name` of the module, or the
    /// module itself when `name` is `None`.
    pub(crate) fn name(&mut self, name: Option<&str>) {
        let Some(name) = name else {
            *self = Self::Every;
            return;
        };
        match self {
            Self::Names(names) => {
                names.insert(name.to_owned());
            }
            Self::Every => {}
        }
    }

    /// What native code sees once the names it writes are followed through
    /// the module's `items`, written in `text`: an item it sees is one its
    /// copy or trampoline declares, so each private item that item's
    /// declaration names is seen too, and so are the methods of a type it
    /// sees, which a native module calls without naming them.
    pub(super) fn closed(&self, text: &str, items: &[item::Parsed]) -> Self {
        let Self::Names(names) = self else {
            return self.clone();
        };
        let mut names = names.clone();
        loop {
            let before = names.len();
            for item in items {
                for declaration in declarations(item, &names) {
                    let declaration = super::slice(text, declaration);
                    names.extend(
                        super::identifiers(declaration)
                            .into_iter()
                            .map(str::to_owned),
                    );
                }
            }
            if names.len() == before {
                return Self::Names(names);
            }
        }
    }
}

/// The spans of what `item` declares that native code sees when it sees
/// `names`: a type's, an alias's or a trait's whole text, a function's
/// signature, and the signatures of the methods of a type it sees.
fn declarations(item: &item::Parsed, names: &BTreeSet<String>) -> Vec<crate::Span> {
    let signature = |function: &item::Function| {
        crate::Span::new(function.span.start(), function.body_span.start())
    };
    match item {
        item::Parsed::Struct(declared) if names.contains(declared.name.text.as_str()) => {
            vec![declared.span]
        }
        item::Parsed::Enum(declared) if names.contains(declared.name.text.as_str()) => {
            vec![declared.span]
        }
        item::Parsed::TypeAlias(declared) if names.contains(declared.name.text.as_str()) => {
            vec![declared.span]
        }
        item::Parsed::Trait(declared) if names.contains(declared.name.text.as_str()) => {
            vec![declared.span]
        }
        item::Parsed::Function(declared) if names.contains(declared.name.text.as_str()) => {
            vec![signature(declared)]
        }
        item::Parsed::Impl(declared)
            if owner(declared).is_some_and(|name| names.contains(name)) =>
        {
            declared.functions.iter().map(signature).collect()
        }
        _ => Vec::new(),
    }
}

/// The name of the type an impl is for, as written: `Item` of `impl Item`.
pub(super) fn owner(declared: &item::Impl) -> Option<&str> {
    match &declared.owner {
        item::type_ref::Type::Path(owner) => {
            owner.segments.last().map(|last| last.name.text.as_str())
        }
        _ => None,
    }
}
