//! The item of a program's type, or the method of a std type, whose name
//! is like one the type misses, which rustc's E0599 suggests: "there is a
//! method `go` with a similar name".

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

    /// The method rustc suggests for `member`, which the std type `ty`,
    /// defined at `definition`, misses: the best of its inherent methods
    /// whose impls fit it and of the methods of std's traits it implements,
    /// a blanket impl's too (`Borrow::borrow`), whose names are within
    /// rustc's reach of `member`. Its place is in std's source, which a
    /// report doesn't show.
    pub(super) fn similar_std_method(
        &self,
        ty: &Ty,
        definition: &str,
        member: &str,
    ) -> Option<Box<Similar>> {
        let reach = similar::reach(member);
        let near = |name: &str| {
            similar::substring_distance(member, name, reach).is_some_and(|score| score > 0)
        };
        let method = |item: &crate::std_table::Item| {
            item.kind == "assoc-fn"
                && item.stability == crate::std_table::Stability::Stable
                && item.receiver().is_some()
        };
        let inherent = crate::std_table::members(definition)
            .filter(|item| method(item) && near(item.name()))
            .filter(|item| std_method::inherent(ty, item.name()).is_some());
        let traits = crate::std_table::trait_function_names()
            .filter(|name| near(name))
            .flat_map(crate::std_table::trait_functions)
            .copied()
            .filter(|item| method(item))
            .filter(|item| {
                item.definition.rsplit_once("::").is_some_and(|(owner, _)| {
                    std_impl::holds(&mut std_impl::Probe(self), owner, ty, &[], true, 0).is_ok()
                })
            });
        let mut candidates: Vec<crate::std_table::Item> = inherent.chain(traits).collect();
        candidates.sort_by_key(|item| item.name());
        candidates.dedup_by_key(|item| item.name());
        let names: Vec<&str> = candidates.iter().map(|item| item.name()).collect();
        let chosen = candidates[similar::best(&names, member)?];
        let parameters =
            crate::std_table::Declared::parse(chosen.declaration()).map_or(0, |declared| {
                crate::std_table::split_list(declared.inputs)
                    .len()
                    .saturating_sub(1)
            });
        let shown = match self.arguments_ahead() {
            Some(count) if count == parameters => Shown::Replaced,
            Some(_) => Shown::DifferentArguments,
            None => Shown::Defined,
        };
        Some(Box::new(Similar {
            name: chosen.name().into(),
            kind: "method",
            declared: Span::NONE,
            shown,
        }))
    }

    /// How many arguments the call the next tokens write gives, without
    /// reading them: `None` where they write no call.
    fn arguments_ahead(&self) -> Option<usize> {
        let mut ahead = self.input.clone();
        match ahead.next()?.0 {
            Token::Unit => return Some(0),
            Token::LeftParen => {}
            _ => return None,
        }
        let (mut depth, mut count, mut open) = (0usize, 0, false);
        for (token, _) in ahead {
            match token {
                Token::LeftParen | Token::LeftBracket | Token::LeftBrace | Token::MapStart => {
                    depth += 1;
                }
                Token::RightParen if depth == 0 => return Some(count + usize::from(open)),
                Token::RightParen | Token::RightBracket | Token::RightBrace => {
                    depth = depth.saturating_sub(1);
                }
                Token::Comma if depth == 0 => {
                    count += 1;
                    open = false;
                    continue;
                }
                _ => {}
            }
            open = true;
        }
        None
    }
}
