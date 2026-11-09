//! A runner's facade, which holds a module the root reaches at more than
//! one path once: `pub use iced_wgpu as wgpu;` in iced_renderer, and the
//! crate's own path below its hidden segment, are one module, and every
//! path below the one is the same path below the other. The other paths
//! are re-exports the runner's registry follows (`Facade::reexports`).

use std::borrow::Cow;
use std::collections::HashMap;

use serde::Serialize;
use sha2::{Digest as _, Sha256};

use super::{table, Facade, ImplHeader, Item, Macro, TraitHeader, TypeAlias, TypeDefinition};
use crate::api::ledger::CRATE_SEGMENT;

/// A facade as a runner's plan writes it: the types, items and named
/// modules below a re-exported module are left to the module it
/// re-exports, and a re-exported variant names its enum there. It reads
/// back as a [`Facade`] with its `reexports`.
#[derive(Serialize)]
pub(super) struct Runner<'facade> {
    root: &'facade str,
    #[serde(serialize_with = "table::serialize_kept")]
    types: Vec<&'facade TypeDefinition>,
    #[serde(serialize_with = "table::serialize_kept")]
    items: Vec<&'facade Item>,
    traits: Vec<&'facade str>,
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    builtins: &'facade [(Box<str>, Box<str>)],
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    variants: Vec<(&'facade str, Cow<'facade, str>)>,
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    type_aliases: &'facade [TypeAlias],
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    macros: &'facade [Macro],
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    derives: &'facade [Box<str>],
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    trait_headers: &'facade [TraitHeader],
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    impls: &'facade [ImplHeader],
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    std_reexports: &'facade [(Box<str>, Box<str>)],
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    reexports: Vec<(&'facade str, &'facade str)>,
}

impl<'facade> Runner<'facade> {
    /// `facade` with each module it reaches at more than one path held at
    /// one of them.
    pub(super) fn new(facade: &'facade Facade) -> Result<Self, serde_json::Error> {
        let reexports = reexports(facade)?;
        let below = |path: &str| {
            path.match_indices("::").any(|(end, _)| {
                reexports
                    .binary_search_by(|(alias, _)| (*alias).cmp(&path[..end]))
                    .is_ok()
            })
        };
        Ok(Self {
            root: &facade.root,
            types: facade.types.iter().filter(|ty| !below(&ty.path)).collect(),
            items: facade
                .items
                .iter()
                .filter(|item| !below(&item.path))
                .collect(),
            traits: facade
                .traits
                .iter()
                .map(AsRef::as_ref)
                .filter(|path| !below(path))
                .collect(),
            builtins: &facade.builtins,
            variants: facade
                .variants
                .iter()
                .map(|(path, variant)| (path.as_ref(), through(&reexports, variant)))
                .collect(),
            type_aliases: &facade.type_aliases,
            macros: &facade.macros,
            derives: &facade.derives,
            trait_headers: &facade.trait_headers,
            impls: &facade.impls,
            std_reexports: &facade.std_reexports,
            reexports,
        })
    }
}

/// `path` as the module each re-exported prefix of it re-exports names
/// it, prefix by prefix, so a variant names its enum where the enum is
/// kept. A re-exported module lies below no re-exported path
/// ([`canonical`]), so each step leaves fewer segments to rebase and the
/// walk ends.
fn through<'path>(reexports: &[(&str, &str)], path: &'path str) -> Cow<'path, str> {
    let mut path = Cow::Borrowed(path);
    loop {
        let found = path.rmatch_indices("::").find_map(|(end, _)| {
            reexports
                .binary_search_by(|(alias, _)| (*alias).cmp(&path[..end]))
                .ok()
                .map(|index| (end, reexports[index].1))
        });
        let Some((end, target)) = found else {
            return path;
        };
        path = Cow::Owned(format!("{target}{}", &path[end..]));
    }
}

/// The modules `facade` reaches at more than one path, each with the path
/// it is kept at, sorted, none below another.
///
/// Two paths are one module when the same entries lie below each, at the
/// same paths below it: a lookup below either finds what it finds below
/// the other, so following the re-export changes no lookup. The path kept
/// is the first by the order a type's paths take (a crate's hidden
/// segment after its public paths, then path order), so every type keeps
/// the first path it had, which names it.
fn reexports(facade: &Facade) -> Result<Vec<(&str, &str)>, serde_json::Error> {
    // A named module that is no type, and a crate's hidden segment.
    let mut digests = HashMap::<&str, (Sha256, usize)>::new();
    for path in facade.traits.iter() {
        if facade
            .types
            .binary_search_by(|ty| (*ty.path).cmp(path))
            .is_err()
        {
            digests.insert(path, (Sha256::new(), 0));
        }
    }
    for path in facade
        .types
        .iter()
        .map(|ty| &*ty.path)
        .chain(facade.items.iter().map(|item| &*item.path))
    {
        if let Some((segment, _)) = path.split_once("::")
            && segment.starts_with(CRATE_SEGMENT)
        {
            digests.entry(segment).or_insert_with(|| (Sha256::new(), 0));
        }
    }
    // Every entry, in a fixed order of kinds and in path order within
    // each, joins the digest of each module above it.
    let mut entries = Vec::<(u8, &str, Vec<u8>)>::new();
    for ty in facade.types.iter() {
        let body = std::sync::Arc::as_ptr(&ty.body) as usize;
        entries.push((0, &ty.path, body.to_le_bytes().to_vec()));
    }
    for item in facade.items.iter() {
        let body = std::sync::Arc::as_ptr(&item.body) as usize;
        entries.push((1, &item.path, body.to_le_bytes().to_vec()));
    }
    for path in facade.traits.iter() {
        entries.push((2, path, Vec::new()));
    }
    for (path, builtin) in facade.builtins.iter() {
        entries.push((3, path, builtin.as_bytes().to_vec()));
    }
    for (path, variant) in facade.variants.iter() {
        entries.push((4, path, variant.as_bytes().to_vec()));
    }
    for alias in facade.type_aliases.iter() {
        let written = serde_json::to_vec(&(&alias.parameters, &alias.defaults, &alias.target))?;
        entries.push((5, &alias.path, written));
    }
    for rules in facade.macros.iter() {
        entries.push((
            6,
            &rules.path,
            [&*rules.source, &*rules.krate].join("\0").into_bytes(),
        ));
    }
    for path in facade.derives.iter() {
        entries.push((7, path, Vec::new()));
    }
    for header in facade.trait_headers.iter() {
        let written =
            serde_json::to_vec(&(&header.identity, &header.parameters, &header.defaults))?;
        entries.push((8, &header.path, written));
    }
    for (path, std) in facade.std_reexports.iter() {
        entries.push((9, path, std.as_bytes().to_vec()));
    }
    // A path's overloads go in the order of their bodies, which a facade
    // sorts again as it installs them.
    entries.sort_unstable();
    for (kind, path, payload) in &entries {
        for (end, _) in path.match_indices("::") {
            let Some((digest, count)) = digests.get_mut(&path[..end]) else {
                continue;
            };
            let below = &path[end + 2..];
            digest.update([*kind]);
            digest.update(below.len().to_le_bytes());
            digest.update(below.as_bytes());
            digest.update(payload.len().to_le_bytes());
            digest.update(payload);
            *count += 1;
        }
    }
    let mut modules = HashMap::<[u8; 32], Vec<&str>>::new();
    for (path, (digest, count)) in digests {
        // An empty module shares nothing with another.
        if count != 0 {
            modules
                .entry(digest.finalize().into())
                .or_default()
                .push(path);
        }
    }
    let mut reexports = Vec::new();
    for mut paths in modules.into_values().filter(|paths| paths.len() > 1) {
        paths.sort_by(|left, right| order(left).cmp(&order(right)));
        let kept = paths[0];
        for path in &paths[1..] {
            // A module that re-exports its own ancestor, or is below its
            // kept path, keeps its entries: a path through it would
            // rebase onto itself.
            if !within(path, kept) && !within(kept, path) {
                reexports.push((*path, kept));
            }
        }
    }
    reexports.sort_unstable();
    // A re-export below another is reached through that one.
    Ok(reexports
        .iter()
        .filter(|(path, _)| {
            !path.match_indices("::").any(|(end, _)| {
                reexports
                    .binary_search_by(|(alias, _)| (*alias).cmp(&path[..end]))
                    .is_ok()
            })
        })
        .copied()
        .collect())
}

/// Where a path goes among one entry's paths: after the paths through no
/// crate's hidden segment, then by its segments.
fn order(path: &str) -> (bool, Vec<&str>) {
    let segments = path.split("::").collect::<Vec<_>>();
    let hidden = segments
        .iter()
        .any(|segment| segment.contains(CRATE_SEGMENT));
    (hidden, segments)
}

/// Whether `path` is `module` or lies below it.
fn within(path: &str, module: &str) -> bool {
    path.strip_prefix(module)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with("::"))
}

/// Whether `reexports` is as [`Runner`] writes it: in path order, and no
/// module one names lies below a re-exported path, so a path through them
/// is rebased in finitely many steps.
pub(super) fn canonical(reexports: &[(Box<str>, Box<str>)]) -> bool {
    reexports.windows(2).all(|pair| pair[0].0 < pair[1].0)
        && reexports.iter().all(|(path, module)| {
            !path.is_empty()
                && !module.is_empty()
                && !reexports
                    .iter()
                    .any(|(above, _)| within(module, above) || within(path, above) && path != above)
        })
}

#[cfg(test)]
mod tests {
    use super::super::Plan;

    /// A module the root reaches at three paths is held at the first of
    /// them by the order a type's paths take; the other two re-export it,
    /// and only a runner's plan has them.
    #[test]
    fn a_module_reached_at_three_paths_is_held_once_and_reexported_twice() {
        let plan = Plan::decode(&crate::test_fixture::runner_reexported_facade_plan()).unwrap();
        let native = plan
            .facades()
            .find(|facade| facade.root() == "native")
            .unwrap();
        assert_eq!(
            native.types().map(|ty| ty.path()).collect::<Vec<_>>(),
            ["renderer::wgpu::Source"]
        );
        assert_eq!(
            native.items().map(|item| item.path()).collect::<Vec<_>>(),
            ["renderer::wgpu::produce"]
        );
        assert_eq!(
            native.reexports().collect::<Vec<_>>(),
            [
                ("__rawr_crate_wgpu_0123456789abcdef", "renderer::wgpu"),
                ("wgpu", "renderer::wgpu"),
            ]
        );
        assert_eq!(
            native.traits().collect::<Vec<_>>(),
            ["renderer", "renderer::wgpu", "wgpu"]
        );
        let full = Plan::decode(&crate::test_fixture::runner_native_facade_plan()).unwrap();
        assert!(full.facades().all(|facade| facade.reexports().len() == 0));
    }

    /// A re-export that names a module below another re-exported path
    /// could rebase without end, so a plan that has one is refused.
    #[test]
    fn a_reexport_naming_a_module_below_another_is_refused() {
        let reexports = [
            (Box::from("a"), Box::from("b::c")),
            (Box::from("b"), Box::from("d")),
        ];
        assert!(!super::canonical(&reexports));
        assert!(super::canonical(&reexports[1..]));
    }
}
