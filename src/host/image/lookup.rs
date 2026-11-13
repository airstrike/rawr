//! Finding a path in a held facade's tables, which its image sorts as a
//! registry sorts paths: segment by segment.

use std::cmp::Ordering;
use std::ops::Range;

use super::Held;
use crate::{host, ImmutableString};
use rawr_import::surface::facade::image::Kind;

/// The order of `written`, an image's path below its root, against
/// `wanted`, a path's segments below its root.
fn compare(written: &str, wanted: &[ImmutableString]) -> Ordering {
    let mut written = host::path::segments(written);
    let mut wanted = wanted.iter();
    loop {
        match (written.next(), wanted.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(written), Some(wanted)) => match written.cmp(wanted.as_str()) {
                Ordering::Equal => {}
                order => return order,
            },
        }
    }
}

/// Whether `written` is `prefix` or lies below it.
fn within(written: &str, prefix: &[ImmutableString]) -> bool {
    let mut written = host::path::segments(written);
    prefix
        .iter()
        .all(|segment| written.next() == Some(segment.as_str()))
}

/// How many segments `written` has.
fn depth(written: &str) -> usize {
    host::path::segments(written).count()
}

/// Where `wanted` is among `count` paths, sorted, `path` giving each.
fn search<'path>(
    count: usize,
    path: impl Fn(usize) -> &'path str,
    wanted: &[ImmutableString],
) -> Result<usize, usize> {
    let start = partition(count, |index| {
        compare(path(index), wanted) == Ordering::Less
    });
    if start < count && compare(path(start), wanted) == Ordering::Equal {
        Ok(start)
    } else {
        Err(start)
    }
}

/// The indices among `count` sorted paths of those that are `prefix` or
/// lie below it.
fn below<'path>(
    count: usize,
    path: impl Fn(usize) -> &'path str,
    prefix: &[ImmutableString],
) -> Range<usize> {
    let start = partition(count, |index| {
        compare(path(index), prefix) == Ordering::Less
    });
    let end = start + partition(count - start, |offset| within(path(start + offset), prefix));
    start..end
}

/// The first of `count` indices at which `before` is false; `before` is
/// true up to some index and false after it.
fn partition(count: usize, before: impl Fn(usize) -> bool) -> usize {
    let (mut low, mut high) = (0, count);
    while low < high {
        let middle = low + (high - low) / 2;
        if before(middle) {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    low
}

/// The direct children among the paths in `range` of the module whose
/// segments below the root number `depth`.
fn children<'path>(
    range: Range<usize>,
    path: impl Fn(usize) -> &'path str,
    module_depth: usize,
) -> impl Iterator<Item = usize> {
    range.filter(move |&index| depth(path(index)) == module_depth + 1)
}

impl Held {
    /// The segments of `path` below this facade's root, when the root is
    /// its first.
    pub(crate) fn below_root<'path>(
        &self,
        path: &'path host::path::Path,
    ) -> Option<&'path [ImmutableString]> {
        let (first, rest) = path.segments().split_first()?;
        (first.as_str() == self.root().name()).then_some(rest)
    }

    /// Whether the facade holds the module at `path`.
    pub(crate) fn module(&self, path: &host::path::Path) -> bool {
        let view = self.view();
        self.below_root(path).is_some_and(|wanted| {
            search(view.modules(), |index| view.module(index), wanted).is_ok()
        })
    }

    /// The index of the type at `path`.
    pub(crate) fn type_index(&self, path: &host::path::Path) -> Option<usize> {
        let view = self.view();
        search(
            view.types(),
            |index| view.type_path(index),
            self.below_root(path)?,
        )
        .ok()
    }

    /// The index of the items at `path`.
    pub(crate) fn group_index(&self, path: &host::path::Path) -> Option<usize> {
        let view = self.view();
        search(
            view.groups(),
            |index| view.group(index).path(),
            self.below_root(path)?,
        )
        .ok()
    }

    /// The items a lookup of `path` finds, when the facade holds some there
    /// apart from a family.
    pub(crate) fn items(&self, path: &host::path::Path) -> Option<&[host::item::Definition]> {
        let items = self.group_at(self.group_index(path)?).items();
        (!items.is_empty()).then_some(items)
    }

    /// The family of distinct imported methods at `path`.
    pub(crate) fn family(&self, path: &host::path::Path) -> Option<&host::family::Pending> {
        self.group_at(self.group_index(path)?).family()
    }

    /// The re-export at `path` and what it names, when it is of a kind
    /// `kinds` takes.
    pub(crate) fn alias(
        &self,
        path: &host::path::Path,
        kinds: impl Fn(Kind) -> bool,
    ) -> Option<&(host::path::Path, host::path::Path)> {
        let view = self.view();
        let index = search(
            view.aliases(),
            |index| view.alias(index).path,
            self.below_root(path)?,
        )
        .ok()?;
        kinds(view.alias(index).kind).then(|| self.alias_at(index))
    }

    /// `path` through the first re-export, in path order, of a kind `kinds`
    /// takes that is `path` or lies above it: the shortest.
    pub(crate) fn through_alias(
        &self,
        path: &host::path::Path,
        kinds: impl Fn(Kind) -> bool,
    ) -> Option<host::path::Path> {
        let segments = path.segments();
        (2..=segments.len()).find_map(|length| {
            let prefix = host::path::Path::from_segments(&segments[..length]);
            let (alias, target) = self.alias(&prefix, &kinds)?;
            path.rebase(alias, target)
        })
    }

    /// The trait header at `path`, when it resolves.
    pub(crate) fn trait_header(
        &self,
        path: &host::path::Path,
    ) -> Option<&host::facade::TraitHeader> {
        let view = self.view();
        let index = search(
            view.trait_headers(),
            |index| view.trait_header_path(index),
            self.below_root(path)?,
        )
        .ok()?;
        self.trait_header_at(index)
    }

    /// Each trait header that resolves, in the facade's order.
    pub(crate) fn trait_headers(&self) -> impl Iterator<Item = &host::facade::TraitHeader> {
        (0..self.view().trait_headers()).filter_map(|index| self.trait_header_at(index))
    }

    /// The impls of the trait `identity` that resolve, in the facade's
    /// order.
    pub(crate) fn impl_headers(&self, identity: &[u8; 32]) -> Vec<&host::facade::ImplHeader> {
        self.view()
            .impls_of(identity)
            .filter_map(|index| self.impl_at(index))
            .filter(|header| header.trait_identity == *identity)
            .collect()
    }

    /// Whether the facade has the derive macro at `path`.
    pub(crate) fn derive(&self, path: &host::path::Path) -> bool {
        let view = self.view();
        self.below_root(path).is_some_and(|wanted| {
            search(view.derives(), |index| view.derive(index), wanted).is_ok()
        })
    }

    /// The indices of the facade's types of `identity`: those at paths
    /// through no crate's hidden segment first, each part in path order.
    pub(crate) fn identity_types(&self, identity: &host::type_def::import::Identity) -> Vec<usize> {
        self.view()
            .identity(identity.as_bytes())
            .map(|types| types.types().collect())
            .unwrap_or_default()
    }

    /// The path of the type at `index`.
    pub(crate) fn type_path(&self, index: usize) -> host::path::Path {
        self.qualified(self.view().type_path(index))
            .expect("a runner image's type paths parse")
    }

    /// Whether the type at `index` is at the qualified `path`, read from
    /// the image's own text: no path is built for it.
    pub(crate) fn type_is_at(&self, index: usize, path: &host::path::Path) -> bool {
        self.below_root(path)
            .is_some_and(|wanted| compare(self.view().type_path(index), wanted) == Ordering::Equal)
    }

    /// Whether the facade declares a crate type at the qualified `path`.
    pub(crate) fn declares_type(&self, path: &str) -> bool {
        host::path::Path::parse(path).is_ok_and(|path| self.type_index(&path).is_some())
    }

    /// The paths directly inside `module` the facade holds something at:
    /// its modules, its types, its items and its re-exports, the last each
    /// with its kind.
    pub(crate) fn inside(&self, module: &host::path::Path) -> Inside {
        let mut inside = Inside::default();
        let Some(prefix) = self.below_root(module) else {
            return inside;
        };
        let view = self.view();
        let module_depth = prefix.len();
        let qualified = |relative: &str| self.qualified(relative).ok();
        let modules = |index| view.module(index);
        inside.modules = children(
            below(view.modules(), modules, prefix),
            modules,
            module_depth,
        )
        .filter_map(|index| qualified(view.module(index)))
        .collect();
        let types = |index| view.type_path(index);
        inside.types = children(below(view.types(), types, prefix), types, module_depth)
            .filter_map(|index| qualified(view.type_path(index)))
            .collect();
        let groups = |index| view.group(index).path();
        inside.items = children(below(view.groups(), groups, prefix), groups, module_depth)
            .filter_map(|index| qualified(view.group(index).path()))
            .collect();
        let aliases = |index| view.alias(index).path;
        inside.aliases = children(
            below(view.aliases(), aliases, prefix),
            aliases,
            module_depth,
        )
        .map(|index| (self.alias_at(index).0.clone(), view.alias(index).kind))
        .collect();
        inside
    }

    /// The items directly under `owner`, as a type's methods, in path
    /// order.
    pub(crate) fn members(&self, owner: &host::path::Path) -> Vec<&host::item::Definition> {
        let Some(prefix) = self.below_root(owner) else {
            return Vec::new();
        };
        let view = self.view();
        let groups = |index| view.group(index).path();
        children(below(view.groups(), groups, prefix), groups, prefix.len())
            .flat_map(|index| self.group_at(index).items())
            .collect()
    }

    /// The facade's free functions named `name`, in path order.
    pub(crate) fn functions_named(&self, name: &str) -> Vec<&host::item::Definition> {
        let mut groups = self.view().named(name).collect::<Vec<_>>();
        groups.sort_unstable();
        groups
            .into_iter()
            .flat_map(|index| self.group_at(index).items())
            .filter(|item| {
                matches!(
                    item.kind(),
                    host::item::Kind::Function | host::item::Kind::IntrinsicFunction
                )
            })
            .collect()
    }

    /// The facade's items and its families' methods of the selector
    /// `fingerprint`: the items in path order, then the methods.
    pub(crate) fn selected(&self, fingerprint: &[u8; 32]) -> Vec<&host::item::Definition> {
        let mut groups = self.view().selected(fingerprint).collect::<Vec<_>>();
        groups.sort_unstable();
        groups.dedup();
        let groups = groups
            .into_iter()
            .map(|index| self.group_at(index))
            .collect::<Vec<_>>();
        let chosen = |item: &&host::item::Definition| {
            item.imported_identity()
                .is_some_and(|identity| identity.fingerprint() == *fingerprint)
        };
        groups
            .iter()
            .flat_map(|group| group.items())
            .filter(chosen)
            .chain(
                groups
                    .iter()
                    .filter_map(|group| group.family())
                    .flat_map(host::family::Pending::methods)
                    .filter(chosen),
            )
            .collect()
    }

    /// The facade's types whose constructor, field or variant has the
    /// selector `fingerprint`, in path order.
    pub(crate) fn structural(&self, fingerprint: &[u8; 32]) -> Vec<&host::type_def::Definition> {
        self.view()
            .structural(fingerprint)
            .map(|index| self.type_at(index))
            .collect()
    }
}

/// What lies directly inside one module of a held facade.
#[derive(Default)]
pub(crate) struct Inside {
    pub(crate) modules: Vec<host::path::Path>,
    pub(crate) types: Vec<host::path::Path>,
    pub(crate) items: Vec<host::path::Path>,
    pub(crate) aliases: Vec<(host::path::Path, Kind)>,
}
