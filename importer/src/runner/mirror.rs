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
//! the runner uses itself, nothing is placed and the manifest stays as it is.
//!
//! The packages rawr generates beside the runner, as its copy of the
//! program's crate, name the project's path packages by their absolute
//! paths, as the import request does. The runner names a copy of each under
//! its root instead ([`COPIES`], #1777): its manifest names them through the
//! runner's links, and every other entry is a link to the original's, so
//! Cargo sees one path for each path package and one hash.

use std::collections::BTreeSet;
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

    /// Whether `directory`, a package's, is one rawr generates beside the
    /// runner.
    fn beside(&self, directory: &Path) -> bool {
        normalize(directory).parent() == normalize(self.runner).parent()
    }

    /// `pending`, every package Cargo reaches from them through relative
    /// paths, and the workspace's own path dependencies, all of which must
    /// be placed. `None` when one can't be.
    fn closure(&self, mut pending: Vec<PathBuf>) -> Option<BTreeSet<PathBuf>> {
        let root = read(&self.workspace.join("Cargo.toml"))?;
        if let Some(dependencies) = root
            .get("workspace")
            .and_then(|workspace| workspace.get("dependencies"))
            .and_then(Item::as_table_like)
        {
            for (_, dependency) in dependencies.iter() {
                if let Some(path) = dependency.as_table_like().and_then(relative_path) {
                    pending.push(self.under(&normalize(&self.workspace.join(path)))?);
                }
            }
        }
        let mut seen = BTreeSet::new();
        while let Some(relative) = pending.pop() {
            if !seen.insert(relative.clone()) {
                continue;
            }
            let directory = self.workspace.join(&relative);
            let Some(mut package) = read(&directory.join("Cargo.toml")) else {
                continue;
            };
            let mut reached = Vec::new();
            for_each_dependency(package.as_table_mut(), &mut |dependency| {
                if let Some(path) = relative_path(dependency) {
                    reached.push(normalize(&directory.join(path)));
                }
            });
            for path in reached {
                pending.push(self.under(&path)?);
            }
        }
        Some(seen)
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

    /// The copy under the runner of `directory`, a package rawr generates
    /// beside it.
    fn copy(&self, directory: &Path) -> Option<PathBuf> {
        Some(self.runner.join(COPIES).join(directory.file_name()?))
    }

    /// Write the copy of each of `generated`, each package's manifest with
    /// its path dependencies named through the runner's links and the other
    /// copies, and every other entry of its directory a link to the
    /// original, so its sources, build script and assets are the
    /// original's. A copy no package needs goes.
    fn write_copies(&self, generated: &[PathBuf]) -> Option<()> {
        let copies = self.runner.join(COPIES);
        fs::create_dir_all(&copies).ok()?;
        let named = generated
            .iter()
            .filter_map(|directory| directory.file_name().map(OsString::from))
            .collect::<BTreeSet<_>>();
        for entry in fs::read_dir(&copies).ok()?.flatten() {
            if !named.contains(&entry.file_name()) {
                fs::remove_dir_all(entry.path()).ok()?;
            }
        }
        for directory in generated {
            let mut package = read(&directory.join("Cargo.toml"))?;
            // Each path from the copy's directory: another copy, a link, or
            // the absolute path.
            for_each_dependency(package.as_table_mut(), &mut |dependency| {
                let Some(path) = dependency.get("path").and_then(Item::as_str) else {
                    return;
                };
                let path = normalize(&directory.join(path));
                let renamed = if generated.contains(&path) {
                    self.copy(&path).unwrap_or(path)
                } else {
                    self.relative(&path)
                        .map_or(path, |relative| self.runner.join(relative))
                };
                dependency.insert("path", toml_edit::value(renamed.display().to_string()));
            });
            // It is a member of the runner's workspace now.
            package.remove("workspace");
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
    let mut document = manifest.parse::<DocumentMut>().ok()?;

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
            if let Some(copy) = root.copy(path) {
                let relative = copy.strip_prefix(root.runner).unwrap_or(&copy);
                dependency.insert("path", toml_edit::value(slashed(relative)));
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

    // The generated packages' own path dependencies are placed too; those
    // they reach beside the runner are generated as well.
    let mut pending = generated.clone();
    while let Some(directory) = pending.pop() {
        let mut package = read(&directory.join("Cargo.toml"))?;
        for_each_dependency(package.as_table_mut(), &mut |dependency| {
            let Some(path) = dependency.get("path").and_then(Item::as_str) else {
                return;
            };
            let path = normalize(&directory.join(path));
            if root.beside(&path) {
                if !generated.contains(&path) {
                    generated.push(path.clone());
                    pending.push(path);
                }
            } else if let Some(relative) = root.relative(&path) {
                placed.push(relative);
            }
        });
    }
    let seen = root.closure(placed)?;

    // Any other package the runner names by its own path that names a
    // placed one by its absolute path would make Cargo see that package at
    // two paths.
    for directory in &unplaced {
        let Some(mut package) = read(&directory.join("Cargo.toml")) else {
            continue;
        };
        let mut named = false;
        for_each_dependency(package.as_table_mut(), &mut |dependency| {
            let Some(path) = dependency.get("path").and_then(Item::as_str) else {
                return;
            };
            named |= root
                .under(&normalize(&directory.join(path)))
                .is_some_and(|relative| seen.iter().any(|placed| relative.starts_with(placed)));
        });
        if named {
            return None;
        }
    }

    root.link(&seen)?;
    root.write_copies(&generated)?;

    // The tables a placed member inherits, as the project's workspace has them.
    let project = read(&root.workspace.join("Cargo.toml"))?;
    if let Some(inherited) = project.get("workspace").and_then(Item::as_table_like) {
        let table = document
            .get_mut("workspace")
            .and_then(Item::as_table_like_mut)?;
        for key in INHERITED {
            if let Some(item) = inherited.get(key) {
                table.insert(key, item.clone());
            }
        }
    }
    Some(document.to_string())
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

fn read(manifest: &Path) -> Option<DocumentMut> {
    fs::read_to_string(manifest).ok()?.parse().ok()
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
}
