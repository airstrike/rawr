//! The project's path packages, placed under the runner's workspace root at
//! the paths they have under the project's (T277).
//!
//! Cargo hashes a path package by its path relative to the workspace root
//! when it lies under that root, and by its absolute path otherwise. The
//! runner's root is `<target>/rawr/runner`, so a path dependency the harness
//! names by its absolute path would get a different `-C metadata` there than
//! in the project's own `cargo build`, and compile a second time. A symbolic
//! link in the runner's directory for each of the project's top-level
//! directories that hold such a package, and a manifest that names the
//! package through it, give Cargo the same relative path in both builds.
//!
//! Cargo joins a relative path lexically, so a package reached through a
//! link that names another by a relative path must find it through a link
//! too. When one would land outside the project's workspace, or on a name
//! the runner uses itself, no path package is placed and the manifest names
//! them as it did. A workspace dependency outside the workspace needs no
//! place: both builds name it by its absolute path.
//!
//! The packages rawr generates beside the runner, as its copy of the
//! program's crate, name the project's path packages by their absolute
//! paths, as the import request does. The runner names a copy of each under
//! its root instead ([`COPIES`], #1777): its manifest names them through the
//! runner's links, and every other entry is a link to the original's, so
//! Cargo sees one path for each path package and one hash. The copies are
//! there even when no path package is placed: under the runner's root they
//! are members of its workspace, so the program's crate builds with the
//! profile `cargo build` gives it, not with the project's settings for
//! packages outside the workspace (#1789).
//!
//! A workspace root that is a package itself, as `icedxdy = { path = "." }`
//! names it, can't be placed at its own path: the runner is its workspace's
//! root. The runner names a copy of it under its root ([`ROOT`]), as it does
//! the generated packages, so the root's path dependencies are placed and it
//! is a member of the runner's workspace (#1913). When no path package can be
//! placed, each path package under the project's workspace, a member of
//! `cargo build`'s workspace but not of the runner's, gets the members' dev
//! profile in place of the project's `[profile.dev.package."*"]`.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};

use toml_edit::{DocumentMut, Item, TableLike};

/// The names the runner's own directory uses.
const RUNNER: [&str; 9] = [
    "Cargo.toml",
    "Cargo.lock",
    "build.rs",
    "src",
    crate::bridge::generate::member::DIRECTORY,
    "target",
    "rawr-generated-manifest.carrier",
    "incremental-directories",
    COPIES,
];

/// The directory under the runner's root that holds its copies of the
/// packages rawr generates beside it (#1777).
pub(crate) const COPIES: &str = "rawr-copies";

/// The name of the copy under [`COPIES`] of the project's workspace root,
/// when that root is a package (#1913).
const ROOT: &str = "rawr-workspace-root";

/// The dependency tables of a manifest or of one `target.<cfg>` table.
const DEPENDENCIES: [&str; 3] = ["dependencies", "build-dependencies", "dev-dependencies"];

/// The `[workspace]` tables a member package inherits from.
const INHERITED: [&str; 3] = ["package", "dependencies", "lints"];

/// `manifest`, the runner's, with each path dependency under `workspace`
/// named through a link in `runner` at its path under `workspace`, and the
/// links in place. A package rawr generates beside the runner, as its copy
/// of the program's crate, is named through its copy under the runner
/// ([`COPIES`]). The project's `[workspace]` tables a member inherits from
/// come along. Returns `manifest` unchanged when the packages can't all be
/// placed.
#[must_use]
pub fn mirror(workspace: &Path, runner: &Path, manifest: &str) -> String {
    place(workspace, runner, manifest).unwrap_or_else(|| manifest.to_owned())
}

/// The path packages one placement names through the runner: those it
/// links, by their paths under the workspace, and whether the workspace's
/// root package is among them, named through its copy.
struct Placed {
    linked: BTreeSet<PathBuf>,
    root: bool,
}

/// The project's workspace root, and the runner's directory below it.
struct Root<'a> {
    workspace: PathBuf,
    runner: &'a Path,
    /// The top-level directory under the workspace that holds the runner.
    inside: Option<OsString>,
}

impl<'a> Root<'a> {
    fn new(workspace: &Path, runner: &'a Path) -> Option<Self> {
        let workspace = fs::canonicalize(workspace).ok()?;
        fs::create_dir_all(runner).ok()?;
        let inside = fs::canonicalize(runner).ok().and_then(|runner| {
            let inside = runner.strip_prefix(&workspace).ok()?;
            Some(inside.components().next()?.as_os_str().to_owned())
        });
        Some(Self {
            workspace,
            runner,
            inside,
        })
    }

    /// A name the runner uses, or the directory that holds the runner.
    fn reserved(&self, name: &str) -> bool {
        RUNNER.contains(&name) || self.inside.as_deref().is_some_and(|inside| inside == name)
    }

    /// `path` under the workspace, when it can be placed.
    fn under(&self, path: &Path) -> Option<PathBuf> {
        let relative = path.strip_prefix(&self.workspace).ok()?;
        let first = relative.components().next()?.as_os_str().to_str()?;
        (!self.reserved(first)).then(|| relative.to_path_buf())
    }

    /// The absolute `path` under the workspace, as written or as it resolves.
    fn relative(&self, path: &Path) -> Option<PathBuf> {
        self.under(&normalize(path)).or_else(|| {
            fs::canonicalize(path)
                .ok()
                .and_then(|path| self.under(&path))
        })
    }

    /// Whether `path`, written or as it resolves, is the workspace's root.
    fn is_workspace(&self, path: &Path) -> bool {
        normalize(path) == self.workspace
            || fs::canonicalize(path).is_ok_and(|path| path == self.workspace)
    }

    /// Whether `directory`, a package's, is one rawr generates beside the
    /// runner.
    fn beside(&self, directory: &Path) -> bool {
        normalize(directory).parent() == normalize(self.runner).parent()
    }

    /// `pending`, every package Cargo reaches from them through relative
    /// paths, and the workspace's own path dependencies under the workspace,
    /// all of which must be placed. `None` when one can't be. A workspace
    /// dependency outside the workspace goes to `beyond`: Cargo hashes it by
    /// its absolute path in both builds, and the runner's copy of the table
    /// names it so ([`inherit`]). The workspace's root package, the empty
    /// path, is reached like any other and placed through its copy.
    fn closure(
        &self,
        manifests: &mut Manifests,
        mut pending: Vec<PathBuf>,
        beyond: &mut Vec<PathBuf>,
    ) -> Option<Placed> {
        let place = |path: &Path| {
            if *path == self.workspace {
                Some(PathBuf::new())
            } else {
                self.under(path)
            }
        };
        let root = manifests.of(&self.workspace)?;
        if let Some(dependencies) = root
            .get("workspace")
            .and_then(|workspace| workspace.get("dependencies"))
            .and_then(Item::as_table_like)
        {
            for (_, dependency) in dependencies.iter() {
                if let Some(path) = dependency.as_table_like().and_then(relative_path) {
                    let path = normalize(&self.workspace.join(path));
                    if path.starts_with(&self.workspace) {
                        pending.push(place(&path)?);
                    } else {
                        beyond.push(path);
                    }
                }
            }
        }
        let mut seen = BTreeSet::new();
        while let Some(relative) = pending.pop() {
            if !seen.insert(relative.clone()) {
                continue;
            }
            let directory = normalize(&self.workspace.join(&relative));
            let Some(package) = manifests.of(&directory) else {
                continue;
            };
            // The root's `[patch]` and `[replace]` are the runner's own,
            // named by their absolute paths, and its copy has none.
            let mut package = package.as_table().clone();
            if relative.as_os_str().is_empty() {
                package.remove("patch");
                package.remove("replace");
            }
            let mut reached = Vec::new();
            visit_dependencies(&package, &mut |dependency| {
                if let Some(path) = relative_path(dependency) {
                    reached.push(normalize(&directory.join(path)));
                }
            });
            for path in reached {
                pending.push(place(&path)?);
            }
        }
        let root = seen.remove(Path::new(""));
        Some(Placed { linked: seen, root })
    }

    /// One link per top-level directory of `seen`, and none left from
    /// another manifest.
    fn link(&self, seen: &BTreeSet<PathBuf>) -> Option<()> {
        let names = seen
            .iter()
            .filter_map(|relative| relative.components().next())
            .map(|first| first.as_os_str().to_owned())
            .collect::<BTreeSet<_>>();
        for entry in fs::read_dir(self.runner).ok()?.flatten() {
            let name = entry.file_name();
            if entry.file_type().ok()?.is_symlink()
                && (!names.contains(&name)
                    || fs::read_link(entry.path()).ok()? != self.workspace.join(&name))
            {
                fs::remove_file(entry.path()).ok()?;
            }
        }
        for name in &names {
            let link = self.runner.join(name);
            match fs::symlink_metadata(&link) {
                Ok(metadata) if metadata.file_type().is_symlink() => {}
                Ok(_) => return None,
                Err(_) => link_directory(&self.workspace.join(name), &link).ok()?,
            }
        }
        Some(())
    }

    /// `generated`, packages rawr generates beside the runner, and those
    /// they reach beside it.
    fn generated(
        &self,
        manifests: &mut Manifests,
        mut generated: Vec<PathBuf>,
    ) -> Option<Vec<PathBuf>> {
        let mut pending = generated.clone();
        while let Some(directory) = pending.pop() {
            let package = manifests.of(&directory)?;
            visit_dependencies(package.as_table(), &mut |dependency| {
                let Some(path) = dependency.get("path").and_then(Item::as_str) else {
                    return;
                };
                let path = normalize(&directory.join(path));
                if self.beside(&path) && !generated.contains(&path) {
                    generated.push(path.clone());
                    pending.push(path);
                }
            });
        }
        Some(generated)
    }

    /// The copy under the runner of `directory`, a package rawr generates
    /// beside it or the workspace's root package.
    fn copy(&self, directory: &Path) -> Option<PathBuf> {
        let name = if *directory == self.workspace {
            std::ffi::OsStr::new(ROOT)
        } else {
            directory.file_name()?
        };
        Some(self.runner.join(COPIES).join(name))
    }

    /// The names of the path packages under the workspace that `document`,
    /// the runner's manifest, and the copies of `generated` reach by their
    /// absolute paths, and those they reach in turn under the workspace.
    fn apart(
        &self,
        manifests: &mut Manifests,
        document: &DocumentMut,
        generated: &[PathBuf],
    ) -> BTreeSet<String> {
        let mut pending = Vec::new();
        let mut reach = |directory: &Path, dependency: &dyn TableLike| {
            let Some(path) = dependency.get("path").and_then(Item::as_str) else {
                return;
            };
            let path = normalize(&directory.join(path));
            if !self.beside(&path) && (self.is_workspace(&path) || self.relative(&path).is_some()) {
                pending.push(path);
            }
        };
        visit_dependencies(document.as_table(), &mut |dependency| {
            reach(self.runner, dependency);
        });
        for directory in generated {
            if let Some(package) = manifests.of(directory) {
                visit_dependencies(package.as_table(), &mut |dependency| {
                    reach(directory, dependency);
                });
            }
        }
        let mut seen = BTreeSet::new();
        let mut names = BTreeSet::new();
        while let Some(directory) = pending.pop() {
            if !seen.insert(directory.clone()) {
                continue;
            }
            let Some(package) = manifests.of(&directory) else {
                continue;
            };
            if let Some(name) = package
                .get("package")
                .and_then(|package| package.get("name"))
                .and_then(Item::as_str)
            {
                names.insert(name.to_owned());
            }
            visit_dependencies(package.as_table(), &mut |dependency| {
                if let Some(path) = relative_path(dependency) {
                    let path = normalize(&directory.join(path));
                    if self.is_workspace(&path) || self.relative(&path).is_some() {
                        pending.push(path);
                    }
                }
            });
        }
        names
    }

    /// Write the copy of each of `generated`, and of the workspace's root
    /// package when `placed` holds it: each package's manifest with its path
    /// dependencies named through the other copies and through the runner's
    /// links to the packages `placed` links, and every other entry of its
    /// directory a link to the original, so its sources, build script and
    /// assets are the original's. A copy no package needs goes.
    fn write_copies(
        &self,
        manifests: &mut Manifests,
        generated: &[PathBuf],
        placed: &Placed,
    ) -> Option<()> {
        let copies = self.runner.join(COPIES);
        fs::create_dir_all(&copies).ok()?;
        let mut copied = generated.to_vec();
        if placed.root {
            copied.push(self.workspace.clone());
        }
        let linked = &placed.linked;
        let named = copied
            .iter()
            .filter_map(|directory| self.copy(directory)?.file_name().map(OsString::from))
            .collect::<BTreeSet<_>>();
        for entry in fs::read_dir(&copies).ok()?.flatten() {
            if !named.contains(&entry.file_name()) {
                fs::remove_dir_all(entry.path()).ok()?;
            }
        }
        for directory in &copied {
            // The copy's own, which its rewrite makes of the original's.
            let mut package = manifests.of(directory)?.clone();
            // Each path from the copy's directory: another copy, a link, or
            // the absolute path.
            for_each_dependency(package.as_table_mut(), &mut |dependency| {
                let Some(path) = dependency.get("path").and_then(Item::as_str) else {
                    return;
                };
                let path = normalize(&directory.join(path));
                let renamed = if copied.contains(&path) {
                    self.copy(&path).unwrap_or(path)
                } else {
                    self.relative(&path)
                        .filter(|relative| linked.iter().any(|placed| relative.starts_with(placed)))
                        .map_or(path, |relative| self.runner.join(relative))
                };
                dependency.insert("path", toml_edit::value(renamed.display().to_string()));
            });
            // It is a member of the runner's workspace now, whose root holds
            // the policy a workspace root's manifest has.
            package.remove("workspace");
            if *directory == self.workspace {
                for key in ["patch", "replace", "profile"] {
                    package.remove(key);
                }
            }
            let copy = self.copy(directory)?;
            fs::create_dir_all(&copy).ok()?;
            let manifest = package.to_string();
            let target = copy.join("Cargo.toml");
            if fs::read_to_string(&target).ok().as_deref() != Some(manifest.as_str()) {
                fs::write(&target, manifest).ok()?;
            }
            let entries = fs::read_dir(directory)
                .ok()?
                .flatten()
                .map(|entry| entry.file_name())
                .filter(|name| name != "Cargo.toml" && name != "Cargo.lock" && name != "target")
                .filter(|name| *directory != self.workspace || self.inside.as_ref() != Some(name))
                .collect::<BTreeSet<_>>();
            for entry in fs::read_dir(&copy).ok()?.flatten() {
                let name = entry.file_name();
                if name != "Cargo.toml"
                    && (!entries.contains(&name)
                        || fs::read_link(entry.path()).ok()? != directory.join(&name))
                {
                    fs::remove_file(entry.path()).ok()?;
                }
            }
            for name in &entries {
                let link = copy.join(name);
                if fs::symlink_metadata(&link).is_err() {
                    link_entry(&directory.join(name), &link).ok()?;
                }
            }
        }
        Some(())
    }
}

fn place(workspace: &Path, runner: &Path, manifest: &str) -> Option<String> {
    let root = Root::new(workspace, runner)?;
    let document = manifest.parse::<DocumentMut>().ok()?;
    let mut manifests = Manifests::default();
    linked(&root, &mut manifests, document.clone())
        .or_else(|| copied(&root, &mut manifests, document))
}

/// `manifest` with its path dependencies under the project's workspace named
/// through the runner's links, and the packages rawr generates beside the
/// runner through their copies. `None` when any can't be placed.
fn linked(root: &Root<'_>, manifests: &mut Manifests, mut document: DocumentMut) -> Option<String> {
    // The runner's path dependencies under the project's workspace, and the
    // packages rawr generates beside the runner.
    let mut placed = Vec::new();
    let mut unplaced = Vec::new();
    let mut generated = Vec::new();
    for_each_dependency(document.as_table_mut(), &mut |dependency| {
        let Some(path) = dependency.get("path").and_then(Item::as_str) else {
            return;
        };
        let path = Path::new(path);
        if !path.is_absolute() {
            return;
        }
        if root.beside(path) {
            generated.push(normalize(path));
            if let Some(copy) = copy_path(root, path) {
                dependency.insert("path", toml_edit::value(copy));
            }
            return;
        }
        if root.is_workspace(path) {
            placed.push(PathBuf::new());
            if let Some(copy) = copy_path(root, &root.workspace) {
                dependency.insert("path", toml_edit::value(copy));
            }
            return;
        }
        match root.relative(path) {
            Some(relative) => {
                placed.push(relative.clone());
                dependency.insert("path", toml_edit::value(slashed(&relative)));
            }
            None => unplaced.push(normalize(path)),
        }
    });
    if placed.is_empty() {
        return None;
    }
    // A package the runner keeps out of its workspace is kept out at the
    // path its dependency now names.
    if let Some(exclude) = document
        .get_mut("workspace")
        .and_then(|workspace| workspace.get_mut("exclude"))
        .and_then(Item::as_array_mut)
    {
        for entry in exclude.iter_mut() {
            let Some(path) = entry
                .as_str()
                .map(Path::new)
                .filter(|path| path.is_absolute())
            else {
                continue;
            };
            if let Some(relative) = root.relative(path) {
                *entry = slashed(&relative).into();
            }
        }
    }

    // The generated packages' own path dependencies are placed too.
    let generated = root.generated(manifests, generated)?;
    for directory in &generated {
        let package = manifests.of(directory)?;
        visit_dependencies(package.as_table(), &mut |dependency| {
            let Some(path) = dependency.get("path").and_then(Item::as_str) else {
                return;
            };
            let path = normalize(&directory.join(path));
            if root.beside(&path) {
                return;
            }
            if root.is_workspace(&path) {
                placed.push(PathBuf::new());
            } else if let Some(relative) = root.relative(&path) {
                placed.push(relative);
            }
        });
    }
    let seen = root.closure(manifests, placed, &mut unplaced)?;

    // Any other package the runner names by its own path that names a
    // placed one by its absolute path would make Cargo see that package at
    // two paths.
    for directory in &unplaced {
        let Some(package) = manifests.of(directory) else {
            continue;
        };
        let mut named = false;
        visit_dependencies(package.as_table(), &mut |dependency| {
            let Some(path) = dependency.get("path").and_then(Item::as_str) else {
                return;
            };
            let path = normalize(&directory.join(path));
            named |= (seen.root && root.is_workspace(&path))
                || root.under(&path).is_some_and(|relative| {
                    seen.linked
                        .iter()
                        .any(|placed| relative.starts_with(placed))
                });
        });
        if named {
            return None;
        }
    }

    root.link(&seen.linked)?;
    root.write_copies(manifests, &generated, &seen)?;
    inherit(root, manifests, &mut document)?;
    Some(document.to_string())
}

/// `manifest` with only the packages rawr generates beside the runner named
/// through their copies, whose manifests name every other path package by
/// its absolute path, for a project whose path packages can't be placed or
/// that has none. A copy is under the runner's root, so it is a member of
/// the runner's workspace and builds with the profile `cargo build` gives
/// the program's crate, never with the `[profile.*.package."*"]` settings
/// for the dependencies outside it (#1789). The path packages under the
/// project's workspace, members of `cargo build`'s workspace but not of the
/// runner's, get the members' dev profile ([`member_profile`]). `None` when
/// the runner names no generated package.
fn copied(root: &Root<'_>, manifests: &mut Manifests, mut document: DocumentMut) -> Option<String> {
    let mut generated = Vec::new();
    for_each_dependency(document.as_table_mut(), &mut |dependency| {
        let Some(path) = dependency
            .get("path")
            .and_then(Item::as_str)
            .map(Path::new)
            .filter(|path| path.is_absolute() && root.beside(path))
        else {
            return;
        };
        generated.push(normalize(path));
        if let Some(copy) = copy_path(root, path) {
            dependency.insert("path", toml_edit::value(copy));
        }
    });
    if generated.is_empty() {
        return None;
    }
    let generated = root.generated(manifests, generated)?;
    let none = Placed {
        linked: BTreeSet::new(),
        root: false,
    };
    root.link(&none.linked)?;
    root.write_copies(manifests, &generated, &none)?;
    let apart = root.apart(manifests, &document, &generated);
    member_profile(&mut document, &apart);
    Some(document.to_string())
}

/// `document`, the runner's manifest, with each package of `names` at the
/// dev profile Cargo gives the workspace's members: every setting
/// `[profile.dev.package."*"]` changes, at `[profile.dev]`'s value or Cargo's
/// default. A package's own settings stay. `None`, with `document` as it
/// was, when the project's dev profile changes no setting for every package.
fn member_profile(document: &mut DocumentMut, names: &BTreeSet<String>) -> Option<()> {
    let dev = document
        .get_mut("profile")
        .and_then(|profile| profile.get_mut("dev"))
        .and_then(Item::as_table_like_mut)?;
    let every = dev
        .get("package")
        .and_then(|packages| packages.get("*"))
        .and_then(Item::as_table_like)?;
    let settings = every
        .iter()
        .filter_map(|(key, _)| {
            let value = dev.get(key).cloned().or_else(|| dev_default(key))?;
            Some((key.to_owned(), value))
        })
        .collect::<Vec<_>>();
    let packages = dev.get_mut("package").and_then(Item::as_table_like_mut)?;
    for name in names {
        let package = packages
            .entry(name)
            .or_insert_with(|| Item::Table(toml_edit::Table::new()))
            .as_table_like_mut()?;
        for (key, value) in &settings {
            if !package.contains_key(key) {
                package.insert(key, value.clone());
            }
        }
    }
    Some(())
}

/// Cargo's default for the dev profile's `key`, when it has one that holds
/// on every platform.
fn dev_default(key: &str) -> Option<Item> {
    match key {
        "opt-level" => Some(toml_edit::value(0)),
        "debug" | "debug-assertions" | "overflow-checks" => Some(toml_edit::value(true)),
        "codegen-units" => Some(toml_edit::value(256)),
        _ => None,
    }
}

/// The path from the runner's root to the copy of `path`, a package
/// generated beside the runner.
fn copy_path(root: &Root<'_>, path: &Path) -> Option<String> {
    let copy = root.copy(path)?;
    Some(slashed(copy.strip_prefix(root.runner).unwrap_or(&copy)))
}

/// The tables a placed member inherits into `document`'s `[workspace]`, as
/// the project's workspace has them. A workspace dependency outside the
/// project's workspace is named by its absolute path, which Cargo would
/// otherwise join to the runner's root, and the workspace's root package
/// by its copy's.
fn inherit(root: &Root<'_>, manifests: &mut Manifests, document: &mut DocumentMut) -> Option<()> {
    let project = manifests.of(&root.workspace)?;
    let Some(inherited) = project.get("workspace").and_then(Item::as_table_like) else {
        return Some(());
    };
    let table = document
        .get_mut("workspace")
        .and_then(Item::as_table_like_mut)?;
    for key in INHERITED {
        let Some(item) = inherited.get(key) else {
            continue;
        };
        let mut item = item.clone();
        if key == "dependencies"
            && let Some(dependencies) = item.as_table_like_mut()
        {
            for (_, dependency) in dependencies.iter_mut() {
                let Some(dependency) = dependency.as_table_like_mut() else {
                    continue;
                };
                let Some(path) =
                    relative_path(dependency).map(|path| normalize(&root.workspace.join(path)))
                else {
                    continue;
                };
                // The root package is named through its copy (#1913).
                let renamed = if path == root.workspace {
                    copy_path(root, &root.workspace)?
                } else if path.starts_with(&root.workspace) {
                    continue;
                } else {
                    path.display().to_string()
                };
                dependency.insert("path", toml_edit::value(renamed));
            }
        }
        table.insert(key, item);
    }
    Some(())
}

/// Calls `visit` on each dependency entry of `table`'s dependency tables,
/// its `target.<cfg>` tables', its `[patch.<source>]` tables' and its
/// `[replace]` table's.
fn for_each_dependency(table: &mut dyn TableLike, visit: &mut dyn FnMut(&mut dyn TableLike)) {
    for name in DEPENDENCIES {
        entries(table.get_mut(name), visit);
    }
    entries(table.get_mut("replace"), visit);
    if let Some(patches) = table.get_mut("patch").and_then(Item::as_table_like_mut) {
        for (_, patch) in patches.iter_mut() {
            entries(Some(patch), visit);
        }
    }
    if let Some(targets) = table.get_mut("target").and_then(Item::as_table_like_mut) {
        for (_, target) in targets.iter_mut() {
            if let Some(target) = target.as_table_like_mut() {
                for name in DEPENDENCIES {
                    entries(target.get_mut(name), visit);
                }
            }
        }
    }
}

/// Calls `visit` on each dependency entry [`for_each_dependency`] visits,
/// without changing them.
fn visit_dependencies(table: &dyn TableLike, visit: &mut dyn FnMut(&dyn TableLike)) {
    let mut each = |item: Option<&Item>| {
        let Some(entries) = item.and_then(Item::as_table_like) else {
            return;
        };
        for (_, entry) in entries.iter() {
            if let Some(entry) = entry.as_table_like() {
                visit(entry);
            }
        }
    };
    for name in DEPENDENCIES {
        each(table.get(name));
    }
    each(table.get("replace"));
    if let Some(patches) = table.get("patch").and_then(Item::as_table_like) {
        for (_, patch) in patches.iter() {
            each(Some(patch));
        }
    }
    if let Some(targets) = table.get("target").and_then(Item::as_table_like) {
        for (_, target) in targets.iter() {
            if let Some(target) = target.as_table_like() {
                for name in DEPENDENCIES {
                    each(target.get(name));
                }
            }
        }
    }
}

/// Calls `visit` on each entry of a table of dependencies.
fn entries(item: Option<&mut Item>, visit: &mut dyn FnMut(&mut dyn TableLike)) {
    if let Some(entries) = item.and_then(Item::as_table_like_mut) {
        for (_, entry) in entries.iter_mut() {
            if let Some(entry) = entry.as_table_like_mut() {
                visit(entry);
            }
        }
    }
}

/// A dependency's `path` when it is relative.
fn relative_path(dependency: &dyn TableLike) -> Option<&str> {
    dependency
        .get("path")
        .and_then(Item::as_str)
        .filter(|path| Path::new(path).is_relative())
}

/// The manifests one placement reads, each read and parsed once.
#[derive(Default)]
struct Manifests {
    read: BTreeMap<PathBuf, Option<DocumentMut>>,
}

impl Manifests {
    /// The manifest of the package in `directory`, when it reads and parses.
    fn of(&mut self, directory: &Path) -> Option<&DocumentMut> {
        self.read
            .entry(directory.to_path_buf())
            .or_insert_with(|| {
                fs::read_to_string(directory.join("Cargo.toml"))
                    .ok()?
                    .parse()
                    .ok()
            })
            .as_ref()
    }
}

/// `path` with `.` and `..` resolved by name, as Cargo joins a dependency's
/// path.
fn normalize(path: &Path) -> PathBuf {
    let mut normal = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normal.pop();
            }
            other => normal.push(other),
        }
    }
    normal
}

/// A relative path as a manifest spells it on every platform.
fn slashed(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(unix)]
fn link_directory(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn link_directory(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

/// A link at `link` to `target`, a file or a directory.
#[cfg(unix)]
fn link_entry(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn link_entry(target: &Path, link: &Path) -> std::io::Result<()> {
    if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn scratch(label: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("rawr-mirror-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::canonicalize(root).unwrap()
    }

    #[test]
    fn a_path_dependency_under_the_workspace_is_named_through_a_link() {
        let root = scratch("link");
        let project = root.join("app");
        write(
            &project.join("Cargo.toml"),
            "[package]\nname = \"app\"\n\n[workspace]\n\n[workspace.package]\nversion = \"1.2.3\"\n",
        );
        write(
            &project.join("crates/events/Cargo.toml"),
            "[package]\nname = \"events\"\nversion.workspace = true\n\n[dependencies]\nshared = { path = \"../shared\" }\n",
        );
        write(
            &project.join("crates/shared/Cargo.toml"),
            "[package]\nname = \"shared\"\n",
        );
        let runner = project.join("target/rawr/runner");
        let manifest = format!(
            "[workspace]\nresolver = \"3\"\n\n[dependencies]\n\
             events = {{ path = \"{}\" }}\nrawr = {{ path = \"/elsewhere/rawr\" }}\n",
            project.join("crates/events").display()
        );
        let mirrored = super::mirror(&project, &runner, &manifest);
        assert!(
            mirrored.contains("events = { path = \"crates/events\" }"),
            "{mirrored}"
        );
        assert!(
            mirrored.contains("path = \"/elsewhere/rawr\""),
            "{mirrored}"
        );
        assert!(mirrored.contains("version = \"1.2.3\""), "{mirrored}");
        assert_eq!(
            fs::read_link(runner.join("crates")).unwrap(),
            project.join("crates")
        );
        // A second pass changes nothing.
        assert_eq!(super::mirror(&project, &runner, &manifest), mirrored);
        fs::remove_dir_all(root).unwrap();
    }

    /// A package the runner keeps out of its workspace, such as the target
    /// of the project's `[patch]`, is excluded at the path its dependency
    /// names through the link.
    #[test]
    fn an_excluded_path_is_named_through_the_link_too() {
        let root = scratch("exclude");
        let project = root.join("app");
        write(
            &project.join("Cargo.toml"),
            "[package]\nname = \"app\"\n\n[workspace]\n",
        );
        write(
            &project.join("shell/Cargo.toml"),
            "[package]\nname = \"shell\"\n",
        );
        let runner = project.join("target/rawr/runner");
        let shell = project.join("shell");
        let manifest = format!(
            "[workspace]\nexclude = [\"{0}\"]\n\n[dependencies]\nshell = {{ path = \"{0}\" }}\n",
            shell.display()
        );
        let mirrored = super::mirror(&project, &runner, &manifest);
        assert!(mirrored.contains("exclude = [\"shell\"]"), "{mirrored}");
        assert!(
            mirrored.contains("shell = { path = \"shell\" }"),
            "{mirrored}"
        );
        fs::remove_dir_all(root).unwrap();
    }

    /// rawr's copy of the program's crate lives beside the runner, outside
    /// its root, and names the project's path dependency by its absolute
    /// path. The runner names the copy's own copy under its root instead,
    /// whose manifest names the dependency through the runner's link: one
    /// package at one path, hashed as `cargo build` hashes it (#1777).
    #[test]
    fn a_package_generated_beside_the_runner_is_named_through_its_copy() {
        let root = scratch("copy");
        let project = root.join("app");
        write(
            &project.join("Cargo.toml"),
            "[package]\nname = \"app\"\n\n[workspace]\n",
        );
        write(
            &project.join("events/Cargo.toml"),
            "[package]\nname = \"events\"\n",
        );
        let generated = project.join("target/rawr/crate");
        write(
            &generated.join("Cargo.toml"),
            &format!(
                "[package]\nname = \"app\"\n\n[lib]\npath = \"src/lib.rs\"\n\n\
                 [dependencies]\nevents = {{ path = \"{}\" }}\n\n[workspace]\n",
                project.join("events").display()
            ),
        );
        write(&generated.join("src/lib.rs"), "");
        let runner = project.join("target/rawr/runner");
        let manifest = format!(
            "[workspace]\n\n[dependencies]\napp = {{ path = \"{}\" }}\nevents = {{ path = \"{}\" }}\n",
            generated.display(),
            project.join("events").display()
        );
        let mirrored = super::mirror(&project, &runner, &manifest);
        assert!(
            mirrored.contains("app = { path = \"rawr-copies/crate\" }"),
            "{mirrored}"
        );
        assert!(
            mirrored.contains("events = { path = \"events\" }"),
            "{mirrored}"
        );
        let copy = runner.join("rawr-copies/crate");
        let copied = fs::read_to_string(copy.join("Cargo.toml")).unwrap();
        assert!(
            copied.contains(&format!("path = \"{}\"", runner.join("events").display())),
            "{copied}"
        );
        assert!(!copied.contains("[workspace]"), "{copied}");
        assert_eq!(
            fs::read_link(copy.join("src")).unwrap(),
            generated.join("src")
        );
        // A second pass changes nothing.
        assert_eq!(super::mirror(&project, &runner, &manifest), mirrored);
        fs::remove_dir_all(root).unwrap();
    }

    /// A workspace root that is a package, named by its members through
    /// `[workspace.dependencies]` at path ".", as icedxdy's: the runner
    /// names its copy under the runner's root, whose manifest names the
    /// root's path dependency through the runner's link, and the inherited
    /// table names the copy too (#1913).
    #[test]
    fn a_workspace_root_package_is_named_through_its_copy() {
        let root = scratch("root-package");
        let project = root.join("shelf");
        write(
            &project.join("Cargo.toml"),
            "[package]\nname = \"shelf\"\nedition.workspace = true\n\n\
             [dependencies]\ncore.workspace = true\n\n\
             [workspace]\nmembers = [\"core\", \"app\"]\n\n\
             [workspace.package]\nedition = \"2024\"\n\n\
             [workspace.dependencies]\nshelf = { path = \".\" }\ncore = { path = \"core\" }\n\n\
             [patch.crates-io]\nlibc = { path = \"../libc\" }\n",
        );
        write(&project.join("src/lib.rs"), "");
        write(
            &project.join("core/Cargo.toml"),
            "[package]\nname = \"core\"\n",
        );
        write(
            &project.join("app/Cargo.toml"),
            "[package]\nname = \"app\"\n\n[dependencies]\nshelf.workspace = true\n",
        );
        let runner = project.join("target/rawr/members/app/runner");
        let manifest = format!(
            "[workspace]\n\n[dependencies]\napp = {{ path = \"{}\" }}\nshelf = {{ path = \"{}\" }}\n",
            project.join("app").display(),
            project.display()
        );
        let mirrored = super::mirror(&project, &runner, &manifest);
        assert!(mirrored.contains("app = { path = \"app\" }"), "{mirrored}");
        let copy = "path = \"rawr-copies/rawr-workspace-root\"";
        assert_eq!(mirrored.matches(copy).count(), 2, "{mirrored}");
        assert!(
            mirrored.contains("core = { path = \"core\" }"),
            "{mirrored}"
        );
        let copied = runner.join("rawr-copies/rawr-workspace-root");
        let text = fs::read_to_string(copied.join("Cargo.toml")).unwrap();
        assert!(!text.contains("[workspace"), "{text}");
        assert!(!text.contains("[patch"), "{text}");
        assert_eq!(
            fs::read_link(copied.join("src")).unwrap(),
            project.join("src")
        );
        assert!(fs::symlink_metadata(copied.join("target")).is_err());
        assert_eq!(
            fs::read_link(runner.join("core")).unwrap(),
            project.join("core")
        );
        // A second pass changes nothing.
        assert_eq!(super::mirror(&project, &runner, &manifest), mirrored);
        fs::remove_dir_all(root).unwrap();
    }

    /// A path package under the workspace that can't be placed is a member
    /// of `cargo build`'s workspace but not of the runner's: it gets the
    /// members' dev profile, not the project's `[profile.dev.package."*"]`.
    #[test]
    fn an_unplaced_path_package_gets_the_members_dev_profile() {
        let root = scratch("apart");
        let project = root.join("app");
        write(
            &project.join("Cargo.toml"),
            "[package]\nname = \"app\"\n\n[workspace]\n",
        );
        write(
            &project.join("events/Cargo.toml"),
            "[package]\nname = \"events\"\n\n[dependencies]\nlib = { path = \"../../lib\" }\n",
        );
        let generated = project.join("target/rawr/crate");
        write(
            &generated.join("Cargo.toml"),
            &format!(
                "[package]\nname = \"app\"\n\n[dependencies]\nevents = {{ path = \"{}\" }}\n",
                project.join("events").display()
            ),
        );
        write(&generated.join("src/main.rs"), "");
        let runner = project.join("target/rawr/runner");
        let manifest = format!(
            "[workspace]\n\n[profile.dev]\ndebug = 1\n\n\
             [profile.dev.package.\"*\"]\nopt-level = 3\ndebug = 0\nstrip = true\n\n\
             [dependencies]\napp = {{ path = \"{}\" }}\n",
            generated.display(),
        );
        let mirrored = super::mirror(&project, &runner, &manifest);
        let document = mirrored.parse::<toml_edit::DocumentMut>().unwrap();
        let events = &document["profile"]["dev"]["package"]["events"];
        assert_eq!(events["opt-level"].as_integer(), Some(0), "{mirrored}");
        assert_eq!(events["debug"].as_integer(), Some(1), "{mirrored}");
        assert!(events.get("strip").is_none(), "{mirrored}");
        assert!(
            document["profile"]["dev"]["package"].get("lib").is_none(),
            "{mirrored}"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_package_that_reaches_outside_the_workspace_leaves_the_manifest_alone() {
        let root = scratch("outside");
        let project = root.join("app");
        write(
            &project.join("Cargo.toml"),
            "[package]\nname = \"app\"\n\n[workspace]\n",
        );
        write(
            &project.join("events/Cargo.toml"),
            "[package]\nname = \"events\"\n\n[dependencies]\nlib = { path = \"../../lib\" }\n",
        );
        let runner = project.join("target/rawr/runner");
        let manifest = format!(
            "[workspace]\n\n[dependencies]\nevents = {{ path = \"{}\" }}\n",
            project.join("events").display()
        );
        assert_eq!(super::mirror(&project, &runner, &manifest), manifest);
        assert!(fs::symlink_metadata(runner.join("events")).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    /// A workspace dependency outside the workspace, as inboard's `fount`,
    /// is named by its absolute path in the runner's copy of the table, so a
    /// member that inherits it finds it, and the members under the
    /// workspace are placed all the same (#1789).
    #[test]
    fn a_workspace_dependency_outside_the_workspace_keeps_its_absolute_path() {
        let root = scratch("beyond");
        let project = root.join("app");
        write(
            &project.join("Cargo.toml"),
            "[package]\nname = \"app\"\n\n[workspace]\n\n\
             [workspace.dependencies]\nlib = { path = \"../lib\" }\n",
        );
        write(
            &project.join("events/Cargo.toml"),
            "[package]\nname = \"events\"\n\n[dependencies]\nlib = { workspace = true }\n",
        );
        write(&root.join("lib/Cargo.toml"), "[package]\nname = \"lib\"\n");
        let runner = project.join("target/rawr/runner");
        let manifest = format!(
            "[workspace]\n\n[dependencies]\nevents = {{ path = \"{}\" }}\nlib = {{ path = \"{}\" }}\n",
            project.join("events").display(),
            root.join("lib").display()
        );
        let mirrored = super::mirror(&project, &runner, &manifest);
        assert!(
            mirrored.contains("events = { path = \"events\" }"),
            "{mirrored}"
        );
        let absolute = format!("lib = {{ path = \"{}\" }}", root.join("lib").display());
        assert_eq!(mirrored.matches(&absolute).count(), 2, "{mirrored}");
        assert!(!mirrored.contains("../lib"), "{mirrored}");
        fs::remove_dir_all(root).unwrap();
    }

    /// A project whose path packages can't be placed, or that has none,
    /// still has rawr's copy of its crate under the runner's root, a member
    /// of the runner's workspace: outside it, the project's
    /// `[profile.dev.package."*"]` would build the copy optimized, as inboard's
    /// `opt-level = 3` did (#1789). The copy names the path packages by
    /// their absolute paths.
    #[test]
    fn a_generated_package_is_copied_when_no_path_package_is_placed() {
        let root = scratch("copied");
        let project = root.join("app");
        write(
            &project.join("Cargo.toml"),
            "[package]\nname = \"app\"\n\n[workspace]\n",
        );
        write(
            &project.join("events/Cargo.toml"),
            "[package]\nname = \"events\"\n\n[dependencies]\nlib = { path = \"../../lib\" }\n",
        );
        let generated = project.join("target/rawr/crate");
        write(
            &generated.join("Cargo.toml"),
            &format!(
                "[package]\nname = \"app\"\n\n[dependencies]\nevents = {{ path = \"{}\" }}\n",
                project.join("events").display()
            ),
        );
        write(&generated.join("src/main.rs"), "");
        let runner = project.join("target/rawr/runner");
        let manifest = format!(
            "[workspace]\n\n[dependencies]\napp = {{ path = \"{}\" }}\nevents = {{ path = \"{}\" }}\n",
            generated.display(),
            project.join("events").display()
        );
        let mirrored = super::mirror(&project, &runner, &manifest);
        assert!(
            mirrored.contains("app = { path = \"rawr-copies/crate\" }"),
            "{mirrored}"
        );
        assert!(
            mirrored.contains(&format!(
                "events = {{ path = \"{}\" }}",
                project.join("events").display()
            )),
            "{mirrored}"
        );
        let copied = fs::read_to_string(runner.join("rawr-copies/crate/Cargo.toml")).unwrap();
        assert!(
            copied.contains(&format!("path = \"{}\"", project.join("events").display())),
            "{copied}"
        );
        assert!(fs::symlink_metadata(runner.join("events")).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
