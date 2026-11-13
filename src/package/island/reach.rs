//! Which changes to what native code sees of the marked modules reach a
//! use the running native build's check recorded (D935.3, D936.5): only
//! those need a new native build. The uses are the running binary's, so an
//! addition that swapped never advances them (D936.2).
//!
//! A changed or removed item reaches native code when native code names
//! it, holds its type, or, for a trait impl's member, holds a type its
//! header names. A new item reaches it when native code looked up its name
//! where it now lands: a member name on its type, a name in a scope that
//! glob-imports its module (#1744). A new trait impl or derive reaches it
//! when native code holds the type and uses the trait, as rustc may select
//! the new impl where it selected another. A `Drop` impl and a stand-in
//! reach it unnamed, through a held type or a held trait object.

use rawr_import::uses::Uses;

use super::{listed, member, Entry, Kind, Shape};

impl Shape {
    /// Why native code must be built again when the shape goes from `self`
    /// to `newer`, judged by `uses`, what the running build's native code
    /// uses, in one line; `None` when no change reaches a use. The line
    /// names the item, as [`Shape::change`]'s does.
    #[must_use]
    pub fn used_change(&self, newer: &Self, uses: &Uses) -> Option<String> {
        if self == newer {
            return None;
        }
        if let Some(line) = self.auto_change(newer, |path| uses.holds(path) || uses.names(path)) {
            return Some(line);
        }
        for (path, entry) in &newer.entries {
            let line = match self.entries.get(path) {
                Some(old) if old != entry && self.reaches(path, old, uses) => {
                    format!("`{path}` changed {}", what(entry.kind))
                }
                None if newer.lands(path, entry, uses) => self.landed(newer, path, entry.kind),
                None | Some(_) => continue,
            };
            return Some(self.derived_with(newer, path).unwrap_or(line));
        }
        let removed = self
            .entries
            .iter()
            .filter(|(path, entry)| {
                !newer.entries.contains_key(*path) && self.reaches(path, entry, uses)
            })
            .map(|(path, _)| path.as_str())
            .collect::<Vec<_>>();
        if let Some(first) = removed.first() {
            // Each item gone, without the members of one that went with it.
            let items = removed
                .iter()
                .filter(|path| !removed.iter().any(|owner| member(path, owner)))
                .map(|path| format!("`{path}`"))
                .collect::<Vec<_>>();
            let items = items.iter().map(String::as_str).collect::<Vec<_>>();
            return Some(
                self.derived_with(newer, first)
                    .unwrap_or_else(|| format!("native code can no longer see {}", listed(&items))),
            );
        }
        self.derives.iter().find_map(|(path, old)| {
            let new = newer.derives.get(path)?;
            let held = uses.holds(path) || uses.names(path);
            // A lost derive: native code built against the type may call it.
            let lost = held && !old.is_subset(new);
            // A new one: rustc may select it where native code uses its
            // trait for the type.
            let gained = uses.holds(path)
                && new
                    .difference(old)
                    .any(|derived| uses.uses_trait(last_name(derived)));
            (lost || gained)
                .then(|| self.derive_change(newer, path))
                .flatten()
        })
    }

    /// Whether native code built against `self` reaches its `entry` at
    /// `path`, so a change to it or its removal needs a new native build.
    fn reaches(&self, path: &str, entry: &Entry, uses: &Uses) -> bool {
        if let Some((module, _)) = path.split_once(" uses ") {
            // A re-export: what native code names through it is the item it
            // re-exports, which a path doesn't show.
            return uses.names_within(module) || uses.globs(module);
        }
        match entry.kind {
            Kind::Type => uses.holds(path) || uses.names(path),
            Kind::Implicit => self.implicit(path, uses),
            Kind::Impl => match self.owner(path) {
                Some(owner) => uses.holds(owner),
                // A trait's impl for a native type, as `impl From<&Theme>
                // for Settings`: rustc selects it unnamed.
                None => true,
            },
            Kind::Function | Kind::Constant | Kind::Other => {
                uses.names(path) || (entry.kind == Kind::Other && uses.names_within(path))
            }
        }
    }

    /// Whether the new `entry` at `path` lands where native code looked:
    /// a name native code resolved now finds it, or rustc may select it.
    fn lands(&self, path: &str, entry: &Entry, uses: &Uses) -> bool {
        if let Some((module, _)) = path.split_once(" uses ") {
            // A private import is no name of the module's to a glob of it.
            return entry.text.starts_with("pub") && uses.globs(module);
        }
        match entry.kind {
            Kind::Implicit => self.implicit(path, uses),
            Kind::Impl => {
                let used =
                    trait_of(path, self.owner(path)).is_some_and(|name| uses.uses_trait(name));
                let probed = self.owner(path).is_some_and(|owner| {
                    path.rsplit_once("::")
                        .is_some_and(|(_, name)| uses.probes(owner, name))
                });
                match self.owner(path) {
                    Some(owner) => uses.holds(owner) && (used || probed),
                    None => used,
                }
            }
            _ => match self.owner(path) {
                Some(owner) => {
                    let rest = &path[owner.len() + 2..];
                    let name = rest.rsplit_once("::").map_or(rest, |(_, name)| name);
                    uses.probes(owner, name)
                }
                None => uses.scopes(path),
            },
        }
    }

    /// Whether native code reaches the `Drop` impl or stand-in at `path`:
    /// it holds its type, or for a stand-in, a trait object of its trait.
    fn implicit(&self, path: &str, uses: &Uses) -> bool {
        if let Some((owner, written)) = path.split_once(" as dyn ") {
            return uses.holds(owner) || uses.objects(last_name(written));
        }
        uses.holder(path).is_some()
    }

    /// The line of an item `newer` adds that lands where native code
    /// looked.
    fn landed(&self, newer: &Self, path: &str, kind: Kind) -> String {
        match kind {
            Kind::Implicit => format!("native code can now see `{path}`"),
            Kind::Impl => format!("native code may now select `{path}`"),
            _ if newer.owner(path).is_some() => {
                format!("native code now finds `{path}` where it looked up its name")
            }
            _ => format!("native code now finds `{path}` through a glob import"),
        }
    }

    /// How the derives of the type `path` is or is a member of changed,
    /// when they did: the reason its text changed with them.
    fn derived_with(&self, newer: &Self, path: &str) -> Option<String> {
        newer.derives.keys().find_map(|owner| {
            let own = path
                .strip_prefix(owner.as_str())
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '));
            own.then(|| self.derive_change(newer, owner)).flatten()
        })
    }

    /// The marked type `path` is a member of, the longest that is.
    fn owner(&self, path: &str) -> Option<&str> {
        self.entries
            .iter()
            .filter(|(owner, entry)| entry.kind == Kind::Type && member(path, owner))
            .map(|(owner, _)| owner.as_str())
            .max_by_key(|owner| owner.len())
    }
}

/// What changed of an entry of `kind`, as a restart line says it.
fn what(kind: Kind) -> &'static str {
    match kind {
        Kind::Type => "its fields",
        Kind::Function | Kind::Impl => "its signature",
        Kind::Constant => "its value, which native code reads",
        Kind::Implicit | Kind::Other => "what native code sees of it",
    }
}

/// The trait a trait impl's member at `path` implements, by name: `From`
/// of `physics::Ball::From<palette::Theme>::from`, or of the impl itself
/// when it has no function. `owner` is the marked type it is a member of;
/// a native type's impl starts with the type, as `::Settings::From<..>`.
fn trait_of<'p>(path: &'p str, owner: Option<&str>) -> Option<&'p str> {
    let rest = match owner {
        Some(owner) => &path[owner.len() + 2..],
        None => path.trim_start_matches("::").split_once("::")?.1,
    };
    // A function's name ends the path, lowercase as Rust writes one.
    let instance = match rest.rsplit_once("::") {
        Some((instance, function))
            if function.starts_with(|first: char| first.is_lowercase() || first == '_')
                && function.chars().all(|c| c.is_alphanumeric() || c == '_') =>
        {
            instance
        }
        _ => rest,
    };
    Some(last_name(instance))
}

/// The last name of a written path, without its generic arguments:
/// `Page` of `crate::page::Page`, `From` of `From<palette::Theme>`.
fn last_name(written: &str) -> &str {
    let bare = written.split('<').next().unwrap_or(written).trim();
    bare.rsplit("::").next().unwrap_or(bare)
}
