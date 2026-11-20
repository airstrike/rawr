//! The item of a program's type whose name is like one the type misses,
//! which rustc's E0599 suggests: "there is a method `go` with a similar
//! name".

use super::*;
use crate::resolve::{similar, Shown, Similar};

/// How an E0599's missing item is named.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Named {
    /// `value.member(..)`, which only a method answers.
    Method,
    /// `Type::member`, which any associated function or constant answers.
    Path,
}

impl Parser<'_> {
    /// The item of `owner` rustc suggests for the missing `member`, named
    /// as `named` says, of a call with `arguments` arguments (`None` where
    /// it isn't called): the best of its items whose names are within
    /// rustc's reach of `member`, in the order the program declares them.
    pub(super) fn similar_item(
        &self,
        owner: type_def::Id,
        member: &str,
        named: Named,
        arguments: Option<usize>,
    ) -> Option<Box<Similar>> {
        let reach = similar::reach(member);
        let inherent = self
            .models
            .methods
            .iter()
            .filter(|((id, _), _)| *id == owner)
            .map(|(_, definition)| definition);
        let traits = self
            .models
            .trait_methods
            .iter()
            .filter(|((id, _), _)| *id == owner)
            .flat_map(|(_, definitions)| definitions.iter());
        let mut candidates: Vec<&method::Definition> = inherent
            .chain(traits)
            .filter(|definition| {
                named == Named::Path || (definition.receiver().is_some() && !definition.constant())
            })
            .filter(|definition| {
                similar::substring_distance(member, definition.name(), reach)
                    .is_some_and(|score| score > 0)
            })
            .collect();
        candidates.sort_by_key(|definition| definition.name_span());
        let mut seen = BTreeSet::new();
        candidates.retain(|definition| seen.insert(definition.name()));
        let names: Vec<&str> = candidates
            .iter()
            .map(|definition| definition.name())
            .collect();
        let chosen = candidates[similar::best(&names, member)?];
        let kind = if chosen.constant() {
            "associated constant"
        } else if chosen.receiver().is_some() {
            "method"
        } else {
            "associated function"
        };
        let parameters = chosen.parameters().len();
        let shown = match (kind, arguments) {
            ("associated constant", None | Some(0)) if named == Named::Path => Shown::Replaced,
            ("associated constant", _) => Shown::Defined,
            (_, Some(count)) if count == parameters => Shown::Replaced,
            ("method", Some(_)) => Shown::DifferentArguments,
            _ => Shown::Defined,
        };
        Some(Box::new(Similar {
            name: chosen.name().into(),
            kind,
            declared: chosen.name_span(),
            shown,
        }))
    }
}
