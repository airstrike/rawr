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

use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use toml_edit::{DocumentMut, Item, TableLike};

/// The names the runner's own directory uses.
const RUNNER: [&str; 7] = [
    "Cargo.toml",
    "Cargo.lock",
    "build.rs",
    "src",
    "target",
    "rawr-generated-manifest.carrier",
    "incremental-directories",
];

/// The dependency tables of a manifest or of one `target.<cfg>` table.
const DEPENDENCIES: [&str; 3] = ["dependencies", "build-dependencies", "dev-dependencies"];

/// The `[workspace]` tables a member package inherits from.
const INHERITED: [&str; 3] = ["package", "dependencies", "lints"];

/// `manifest`, the runner's, with each path dependency under `workspace`
/// named through a link in `runner` at its path under `workspace`, and the
/// links in place. The project's `[workspace]` tables a member inherits from
/// come along. Returns `manifest` unchanged when the packages can't all be
/// placed.
#[must_use]
pub fn mirror(workspace: &Path, runner: &Path, manifest: &str) -> String {
    place(workspace, runner, manifest).unwrap_or_else(|| manifest.to_owned())
}

fn place(workspace: &Path, runner: &Path, manifest: &str) -> Option<String> {
    let workspace = fs::canonicalize(workspace).ok()?;
    fs::create_dir_all(runner).ok()?;
    let inside = fs::canonicalize(runner).ok().and_then(|runner| {
        let inside = runner.strip_prefix(&workspace).ok()?;
        Some(inside.components().next()?.as_os_str().to_owned())
    });
    let mut document = manifest.parse::<DocumentMut>().ok()?;
    // A name the runner uses, or the directory that holds the runner.
    let reserved = |name: &str| {
        RUNNER.contains(&name) || inside.as_deref().is_some_and(|inside| inside == name)
    };
    let under = |path: &Path| -> Option<PathBuf> {
        let relative = path.strip_prefix(&workspace).ok()?;
        let first = relative.components().next()?.as_os_str().to_str()?;
        (!reserved(first)).then(|| relative.to_path_buf())
    };

    // The runner's path dependencies under the project's workspace.
    let mut placed = Vec::new();
    for_each_dependency(document.as_table_mut(), &mut |dependency| {
        let Some(path) = dependency.get("path").and_then(Item::as_str) else {
            return;
        };
        let path = Path::new(path);
        if !path.is_absolute() {
            return;
        }
        let relative = under(&normalize(path))
            .or_else(|| fs::canonicalize(path).ok().and_then(|path| under(&path)));
        if let Some(relative) = relative {
            placed.push(relative.clone());
            dependency.insert("path", toml_edit::value(slashed(&relative)));
        }
    });
    if placed.is_empty() {
        return None;
    }

    // Every package Cargo reaches from those through relative paths, and
    // the workspace's own dependencies, must be placed too.
    let root = read(&workspace.join("Cargo.toml"))?;
    let mut seen = BTreeSet::new();
    let mut pending = placed.clone();
    if let Some(dependencies) = root
        .get("workspace")
        .and_then(|workspace| workspace.get("dependencies"))
        .and_then(Item::as_table_like)
    {
        for (_, dependency) in dependencies.iter() {
            if let Some(path) = dependency.as_table_like().and_then(relative_path) {
                pending.push(under(&normalize(&workspace.join(path)))?);
            }
        }
    }
    while let Some(relative) = pending.pop() {
        if !seen.insert(relative.clone()) {
            continue;
        }
        let directory = workspace.join(&relative);
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
            pending.push(under(&path)?);
        }
    }

    // One link per top-level directory, and none left from another manifest.
    let names = seen
        .iter()
        .filter_map(|relative| relative.components().next())
        .map(|first| first.as_os_str().to_owned())
        .collect::<BTreeSet<_>>();
    for entry in fs::read_dir(runner).ok()?.flatten() {
        let name = entry.file_name();
        if entry.file_type().ok()?.is_symlink()
            && (!names.contains(&name)
                || fs::read_link(entry.path()).ok()? != workspace.join(&name))
        {
            fs::remove_file(entry.path()).ok()?;
        }
    }
    for name in &names {
        let link = runner.join(name);
        match fs::symlink_metadata(&link) {
            Ok(metadata) if metadata.file_type().is_symlink() => {}
            Ok(_) => return None,
            Err(_) => link_directory(&workspace.join(name), &link).ok()?,
        }
    }

    // The tables a placed member inherits, as the project's workspace has them.
    if let Some(inherited) = root.get("workspace").and_then(Item::as_table_like) {
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
