//! The items a `use` could bring into scope for a name that resolves to
//! nothing, which rustc suggests importing: the program's own, which the
//! resolver finds, and std's, from the std table.

use crate::{std_table, ImmutableString, Position};

use super::Error;

/// The items a `use` could bring into scope for a name that resolves to
/// nothing, as rustc suggests importing them, and where it puts the `use`.
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct Imports {
    /// Each item, in rustc's order: the program's, then std's, then
    /// core's, each by path.
    pub candidates: Box<[Candidate]>,
    /// Where rustc puts the `use`.
    pub placement: Option<Position>,
}

/// An item a `use` could import: its path and what rustc calls it.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Candidate {
    /// The path the `use` names: `crate::m::f`, `std::collections::HashMap`.
    pub path: ImmutableString,
    /// What rustc calls the item: `function`, `unit struct`, `enum`.
    pub noun: &'static str,
}

/// Where a name that resolves to nothing is used, which decides the items
/// rustc offers to import under it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Wanted {
    /// A value: a function, a unit or tuple struct, a constant.
    Value,
    /// A call's callee: a function or a unit or tuple struct.
    Call,
    /// A type: a struct, an enum, a trait or a type alias.
    Type,
    /// A trait, as a bound or an `impl` names it.
    Trait,
}

impl Imports {
    /// `candidates` in rustc's order, each once, with the `use` at
    /// `placement`. rustc offers core's items only where they aren't as
    /// many as std's, which then holds each of them.
    pub(crate) fn new(mut candidates: Vec<Candidate>, placement: Option<Position>) -> Box<Self> {
        // The program's items, then std's, then core's, then another
        // crate's.
        let crate_of = |candidate: &Candidate| match candidate.path.split("::").next() {
            Some("crate") => 0,
            Some("std") => 1,
            Some("core") => 2,
            _ => 3,
        };
        candidates.sort_by(|a, b| crate_of(a).cmp(&crate_of(b)).then_with(|| a.cmp(b)));
        candidates.dedup();
        let count = |root| {
            candidates
                .iter()
                .filter(|candidate| crate_of(candidate) == root)
                .count()
        };
        if count(1) == count(2) {
            candidates.retain(|candidate| crate_of(candidate) != 2);
        }
        Box::new(Self {
            candidates: candidates.into_boxed_slice(),
            placement,
        })
    }

    /// rustc's help over the `use`s: this item, one of these of a kind, or
    /// one of these items of several kinds. `None` for no candidate.
    pub(crate) fn message(&self) -> Option<String> {
        let first = self.candidates.first()?;
        if self.candidates.len() == 1 {
            return Some(format!("consider importing this {}", first.noun));
        }
        let noun = if self
            .candidates
            .iter()
            .all(|candidate| candidate.noun == first.noun)
        {
            first.noun
        } else {
            "item"
        };
        Some(format!("consider importing one of these {noun}s"))
    }
}

impl Wanted {
    /// What rustc calls the program's struct of `form` it offers.
    pub(crate) const fn struct_noun(self, form: crate::item::StructForm) -> Option<&'static str> {
        use crate::item::StructForm;
        match (self, form) {
            (Self::Type, _) => Some("struct"),
            (Self::Value | Self::Call, StructForm::Unit) => Some("unit struct"),
            (Self::Value | Self::Call, StructForm::Tuple) => Some("tuple struct"),
            _ => None,
        }
    }

    /// What rustc calls a std item of `kind` it offers, `None` for one it
    /// doesn't.
    fn std_noun(self, kind: &str) -> Option<&'static str> {
        match (self, kind) {
            (Self::Value | Self::Call, "fn") => Some("function"),
            (Self::Value | Self::Call, "tuple-struct") => Some("tuple struct"),
            (Self::Value | Self::Call, "unit-struct") => Some("unit struct"),
            (Self::Value, "const") => Some("constant"),
            (Self::Type, "struct") => Some("struct"),
            (Self::Type, "enum") => Some("enum"),
            (Self::Type, "union") => Some("union"),
            (Self::Type | Self::Trait, "trait") => Some("trait"),
            (Self::Type, "type") => Some("type alias"),
            _ => None,
        }
    }
}

/// std's and core's items named `name` a `use` could import where the
/// name is `wanted`.
pub(crate) fn std(name: &str, wanted: Wanted) -> impl Iterator<Item = Candidate> {
    let namespace = match wanted {
        Wanted::Value | Wanted::Call => std_table::Namespace::Value,
        Wanted::Type | Wanted::Trait => std_table::Namespace::Type,
    };
    std_table::named(name, namespace)
        .into_iter()
        .filter_map(move |found| {
            Some(Candidate {
                path: found.path.into(),
                noun: wanted.std_noun(found.kind)?,
            })
        })
}

impl Error {
    /// The error with the items a `use` at `placement` could import under
    /// the bare name it misses, when it offers none yet: `own`, the
    /// program's for the name where it is wanted, and std's.
    #[must_use]
    pub(crate) fn unimported(
        self,
        own: impl FnOnce(&str, Wanted) -> Vec<Candidate>,
        placement: Option<Position>,
    ) -> Self {
        let mut error = self;
        let (name, wanted, imports) = match &mut error {
            Error::UnknownType { name, imports, .. } => (name.clone(), Wanted::Type, imports),
            Error::UnknownVariable { name, imports, .. } => (name.clone(), Wanted::Value, imports),
            Error::UnknownFunction { name, imports, .. } => (name.clone(), Wanted::Call, imports),
            Error::UnknownTrait { name, imports, .. } => (name.clone(), Wanted::Trait, imports),
            _ => return error,
        };
        if name.contains("::") || imports.placement.is_some() || !imports.candidates.is_empty() {
            return error;
        }
        let mut candidates = own(&name, wanted);
        candidates.extend(std(&name, wanted));
        *imports = Imports::new(candidates, placement);
        error
    }
}
