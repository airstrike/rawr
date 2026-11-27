//! A trait's default bodies no impl receives. rustc checks a default body
//! once, at its trait, with `Self` a parameter the trait bounds; rawr reads
//! one an impl receives at that impl's types, and so never read these. Each
//! is read as the method of a check-only `impl<Self: Trait> Trait for
//! Self`, which no type has and no call reaches (#191).

use super::item;
use super::type_ref;
use crate::{program, ImmutableString, Span};
use std::collections::{BTreeMap, BTreeSet};

/// The traits the resolver knows, by name, each with its source.
type Traits = BTreeMap<ImmutableString, (program::source::Id, item::Trait)>;

/// For each of the program's traits with default bodies no impl receives,
/// the check-only `impl<Self: Trait> Trait for Self` whose functions are
/// those bodies.
pub(super) fn shells(items: &[item::Shell], traits: &Traits) -> Vec<item::Shell> {
    let received = received(items, traits);
    items
        .iter()
        .filter_map(|shell| match &shell.kind {
            item::Kind::Trait(declared) if traits.contains_key(&declared.name.text) => {
                check(shell, declared, &received)
            }
            _ => None,
        })
        .collect()
}

/// Where each default body is that no impl receives and no check reads: a
/// std trait's or a facade trait's, whose borrows and moves are left to
/// rustc (#2692).
#[cfg(test)]
pub(super) fn unchecked(items: &[item::Shell], traits: &Traits) -> Vec<Span> {
    let received = received(items, traits);
    traits
        .iter()
        .filter(|(_, (_, declared))| declared.std.is_some() || declared.declaration.is_some())
        .flat_map(|(name, (_, declared))| {
            declared
                .functions
                .iter()
                .zip(declared.provided.iter())
                .filter(|(function, provided)| {
                    **provided && !received.contains(&(name.clone(), function.name.text.clone()))
                })
                .map(|(function, _)| function.name.span)
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Each trait function some impl receives its trait's body of, as
/// `(trait, function)`.
fn received(
    items: &[item::Shell],
    traits: &Traits,
) -> BTreeSet<(ImmutableString, ImmutableString)> {
    let mut received = BTreeSet::new();
    for shell in items {
        let item::Kind::Impl(implementation) = &shell.kind else {
            continue;
        };
        let Some(item::TraitImpl::User { name, .. }) = &implementation.trait_ else {
            continue;
        };
        let Some((_, declared)) = traits.get(&name.text) else {
            continue;
        };
        let overrides = |function: &item::Function| {
            implementation
                .functions
                .iter()
                .any(|own| own.name.text == function.name.text)
        };
        received.extend(
            declared
                .functions
                .iter()
                .filter(|function| !overrides(function))
                .map(|function| (name.text.clone(), function.name.text.clone())),
        );
    }
    received
}

/// The check-only impl of the program's trait `declared`, which `shell`
/// declares, when some of its default bodies no impl receives: `Self` is
/// its first parameter, bounded by the trait and its supertraits'
/// capabilities, and the trait's own parameters follow.
fn check(
    shell: &item::Shell,
    declared: &item::Trait,
    received: &BTreeSet<(ImmutableString, ImmutableString)>,
) -> Option<item::Shell> {
    if declared.std.is_some() || declared.declaration.is_some() || declared.name.span == Span::NONE
    {
        return None;
    }
    let unreceived = |function: &item::Function| {
        !received.contains(&(declared.name.text.clone(), function.name.text.clone()))
    };
    let functions = declared
        .functions
        .iter()
        .zip(declared.provided.iter())
        .filter(|(function, provided)| **provided && unreceived(function))
        .map(|(function, _)| function.clone())
        .collect::<Box<[_]>>();
    if functions.is_empty() {
        return None;
    }
    let arguments = declared
        .generics
        .iter()
        .map(|generic| parameter(&generic.name))
        .collect::<Box<[_]>>();
    let self_name = crate::item::Name {
        text: "Self".into(),
        span: declared.name.span,
    };
    let trait_arguments = if arguments.is_empty() {
        Box::default()
    } else {
        Box::from([(declared.name.text.clone(), arguments.clone())])
    };
    let bounded = item::Generic {
        name: self_name.clone(),
        bounds: declared.supertraits.0,
        written: Box::default(),
        traits: Box::from([declared.name.clone()]),
        written_traits: Box::default(),
        projections: Box::default(),
        trait_arguments,
        default: None,
        host: false,
        crate_bounds: Box::default(),
        native_traits: Box::default(),
        lifted: false,
        constant: None,
        fixed: None,
    };
    let generics = std::iter::once(bounded)
        .chain(declared.generics.iter().cloned())
        .collect();
    Some(item::Shell {
        id: shell.id,
        source: shell.source,
        key: None,
        span: shell.span,
        kind: item::Kind::Impl(item::Impl {
            header: declared.header.clone(),
            generics,
            trait_: Some(item::TraitImpl::User {
                name: declared.name.clone(),
                arguments,
            }),
            owner: parameter(&self_name),
            statics: false,
            functions,
            associated: Box::default(),
            std: None,
        }),
        facades: shell.facades.clone(),
    })
}

/// The type parameter `name`.
fn parameter(name: &crate::item::Name) -> type_ref::Type {
    type_ref::Type::Path(type_ref::Path {
        name: name.text.clone(),
        target: type_ref::Target::Parameter,
        arguments: Box::default(),
        span: name.span,
    })
}
