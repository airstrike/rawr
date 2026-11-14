//! Where a project's Rawr products live.
//!
//! Cargo keeps a project's build output in the project's target directory, and
//! so does Rawr: `<target>/rawr` holds what Rawr builds and keeps for a
//! project, and `<target>/rawr/members/<path>` for a workspace member
//! ([`rawr_directory`]). A target directory several workspaces share keeps
//! each one's under `<target>/rawr/projects/<name>-<key>`. `cargo clean`
//! removes it.

use std::fs;
use std::path::{Component, Path, PathBuf};

use toml_edit::DocumentMut;

/// The project's Cargo target directory: `CARGO_TARGET_DIR`, then
/// `CARGO_BUILD_TARGET_DIR`, then `build.target-dir` from the Cargo config files
/// Cargo reads for `project` (the deepest `.cargo/config.toml` first, then
/// `$CARGO_HOME/config.toml`), else `target` under the workspace root.
///
/// The directory is [`resolved`], so a relative `CARGO_TARGET_DIR` names
/// the same paths as the absolute one.
pub fn target_directory<'a>(
    project: &Path,
    environment: impl Fn(&str) -> Option<&'a str>,
    cargo_home: Option<&Path>,
) -> PathBuf {
    resolved(&configured_target_directory(
        project,
        environment,
        cargo_home,
    ))
}

fn configured_target_directory<'a>(
    project: &Path,
    environment: impl Fn(&str) -> Option<&'a str>,
    cargo_home: Option<&Path>,
) -> PathBuf {
    for key in ["CARGO_TARGET_DIR", "CARGO_BUILD_TARGET_DIR"] {
        if let Some(value) = environment(key).filter(|value| !value.is_empty()) {
            return project.join(value);
        }
    }
    let configs = project
        .ancestors()
        .map(|directory| directory.join(".cargo"))
        .chain(cargo_home.map(Path::to_path_buf));
    for directory in configs {
        for name in ["config.toml", "config"] {
            let Some(value) = configured_target_dir(&directory.join(name)) else {
                continue;
            };
            // A config path is relative to the directory holding `.cargo`.
            let base = if directory.ends_with(".cargo") {
                directory.parent().unwrap_or(&directory).to_path_buf()
            } else {
                directory.clone()
            };
            return base.join(value);
        }
    }
    workspace_root(project).join("target")
}

/// The project's Cargo build directory, where Cargo keeps intermediate
/// artifacts: `CARGO_BUILD_BUILD_DIR`, then `build.build-dir` from the Cargo
/// config files, else the target directory. `{workspace-root}` and
/// `{cargo-cache-home}` expand as Cargo expands them; a path with any other
/// template is Cargo's to place, and the target directory stands for it.
pub fn build_directory<'a>(
    project: &Path,
    environment: impl Fn(&str) -> Option<&'a str> + Copy,
    cargo_home: Option<&Path>,
) -> PathBuf {
    let expand = |value: &str, base: &Path| -> Option<PathBuf> {
        let root = workspace_root(project);
        let value = value.replace("{workspace-root}", &root.to_string_lossy());
        let value = match cargo_home {
            Some(home) => value.replace("{cargo-cache-home}", &home.to_string_lossy()),
            None => value,
        };
        (!value.contains('{')).then(|| resolved(&base.join(value)))
    };
    if let Some(value) = environment("CARGO_BUILD_BUILD_DIR").filter(|value| !value.is_empty()) {
        if let Some(path) = expand(value, project) {
            return path;
        }
        return target_directory(project, environment, cargo_home);
    }
    let configs = project
        .ancestors()
        .map(|directory| directory.join(".cargo"))
        .chain(cargo_home.map(Path::to_path_buf));
    for directory in configs {
        for name in ["config.toml", "config"] {
            let Some(value) = configured(&directory.join(name), "build-dir") else {
                continue;
            };
            let base = if directory.ends_with(".cargo") {
                directory.parent().unwrap_or(&directory).to_path_buf()
            } else {
                directory.clone()
            };
            if let Some(path) = expand(&value, &base) {
                return path;
            }
            return target_directory(project, environment, cargo_home);
        }
    }
    target_directory(project, environment, cargo_home)
}

/// `path` as the filesystem reaches it, spelled without `.` or `..`: each
/// `..` leaves the directory named so far past its symlinks, as the kernel
/// does, and an absent directory is left lexically. Symlinks the path names
/// keep their spelling, so a watch on them still sees them retargeted.
#[must_use]
pub fn resolved(path: &Path) -> PathBuf {
    let mut resolved = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir if resolved.has_root() => {
                if let Ok(physical) = fs::canonicalize(&resolved) {
                    resolved = physical;
                }
                resolved.pop();
            }
            // A relative path keeps the `..` that leaves where it starts.
            Component::ParentDir => {
                if matches!(
                    resolved.components().next_back(),
                    Some(Component::Normal(_))
                ) {
                    resolved.pop();
                } else {
                    resolved.push(component);
                }
            }
            _ => resolved.push(component),
        }
    }
    resolved
}

/// The target triple `cargo build` builds `project` for when none is named:
/// `CARGO_BUILD_TARGET`, then `build.target` from the Cargo config files, else
/// `None` (the host, in `target/<profile>`).
pub fn build_target<'a>(
    project: &Path,
    environment: impl Fn(&str) -> Option<&'a str>,
    cargo_home: Option<&Path>,
) -> Option<String> {
    if let Some(value) = environment("CARGO_BUILD_TARGET").filter(|value| !value.is_empty()) {
        return Some(value.to_owned());
    }
    project
        .ancestors()
        .map(|directory| directory.join(".cargo"))
        .chain(cargo_home.map(Path::to_path_buf))
        .flat_map(|directory| ["config.toml", "config"].map(|name| directory.join(name)))
        .find_map(|path| configured(&path, "target"))
}

/// The Cargo config files Cargo reads for `project` that set a profile for
/// the `rawr` crate by name (`[profile.dev.package.rawr]`). A Rawr checkout's
/// own config does, and a project under it would inherit that.
pub fn rawr_profile_configs(project: &Path, cargo_home: Option<&Path>) -> Vec<PathBuf> {
    project
        .ancestors()
        .map(|directory| directory.join(".cargo"))
        .chain(cargo_home.map(Path::to_path_buf))
        .flat_map(|directory| ["config.toml", "config"].map(|name| directory.join(name)))
        .filter(|path| {
            let Some(document) = fs::read_to_string(path)
                .ok()
                .and_then(|text| text.parse::<DocumentMut>().ok())
            else {
                return false;
            };
            document
                .get("profile")
                .and_then(|profiles| profiles.as_table_like())
                .is_some_and(|profiles| {
                    profiles.iter().any(|(_, profile)| {
                        profile
                            .get("package")
                            .and_then(|packages| packages.as_table_like())
                            .is_some_and(|packages| {
                                packages
                                    .iter()
                                    .any(|(spec, _)| spec == "rawr" || spec.starts_with("rawr@"))
                            })
                    })
                })
        })
        .collect()
}

fn configured_target_dir(path: &Path) -> Option<String> {
    configured(path, "target-dir")
}

fn configured(path: &Path, key: &str) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let document = text.parse::<DocumentMut>().ok()?;
    document.get("build")?.get(key)?.as_str().map(str::to_owned)
}

/// Where Rawr keeps what it builds and remembers for `project`:
/// `<target>/rawr` for a crate of its own or a workspace's root package, and
/// `<target>/rawr/members/<path>` for a member elsewhere in a workspace,
/// `<path>` being the member's directory under the workspace root. Members
/// share the workspace's target directory, as under Cargo, and each keeps
/// its own runner, crate and store.
pub fn rawr_directory<'a>(
    project: &Path,
    environment: impl Fn(&str) -> Option<&'a str>,
    cargo_home: Option<&Path>,
) -> PathBuf {
    rawr_directory_in(&target_directory(project, environment, cargo_home), project)
}

/// [`rawr_directory`] for `project` when its target directory is `target`.
///
/// A target directory other than the workspace's own `target` may be
/// shared, as Cargo allows: a `CARGO_TARGET_DIR` or a `build.target-dir`
/// several projects name. Each workspace then keeps its products under
/// `<target>/rawr/projects/<name>-<key>`, the key a digest of the
/// workspace root's path, so one project never builds from another's crate,
/// runner or store.
#[must_use]
pub fn rawr_directory_in(target: &Path, project: &Path) -> PathBuf {
    let root = physical(&workspace_root(project));
    let rawr = if physical(target) == root.join("target") {
        target.join("rawr")
    } else {
        target.join("rawr").join(PROJECTS).join(project_key(&root))
    };
    match member_path(project) {
        Some(member) => rawr.join("members").join(member),
        None => rawr,
    }
}

/// What tells `project`'s Rawr directory `rawr` ([`rawr_directory_in`])
/// apart from every other project's in its target directory: its path
/// under `<target>/rawr`. `None` for a workspace's root package in the
/// workspace's own target directory, which no other project's shares.
///
/// Cargo hashes a path package by its path under its workspace root, and
/// each project's runner roots a workspace of its own laid out alike, so
/// this is what keeps their units apart (#1984).
#[must_use]
pub fn shared_path(rawr: &Path, project: &Path) -> Option<PathBuf> {
    let member = member_path(project);
    let keyed = Path::new(PROJECTS)
        .join(project_key(&physical(&workspace_root(project))))
        .join(member.as_deref().unwrap_or(Path::new("")));
    if rawr.ends_with(&keyed) {
        Some(keyed)
    } else {
        member
    }
}

/// The directory under `<target>/rawr` that holds each workspace's products
/// when the target directory is not the workspace's own.
pub const PROJECTS: &str = "projects";

/// `<name>-<16 hex digits>` for the workspace at `root`: its directory's
/// name and an FNV-1a digest of its path, the same in every process.
fn project_key(root: &Path) -> String {
    let name = root
        .file_name()
        .map_or_else(|| "root".into(), |name| name.to_string_lossy());
    let digest = root
        .as_os_str()
        .as_encoded_bytes()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
        });
    format!("{name}-{digest:016x}")
}

/// `path` past its symlinks: the path itself when it exists, else its
/// parent's with its last name, else `path` as given.
fn physical(path: &Path) -> PathBuf {
    if let Ok(path) = fs::canonicalize(path) {
        return path;
    }
    path.parent()
        .zip(path.file_name())
        .and_then(|(parent, name)| {
            fs::canonicalize(parent)
                .ok()
                .map(|parent| parent.join(name))
        })
        .unwrap_or_else(|| path.to_path_buf())
}

/// `project`'s directory under its workspace root, when it is a member of
/// a workspace rooted elsewhere. Cargo builds such a member with the
/// toolchain and the config files the workspace root's directory selects.
#[must_use]
pub fn member_path(project: &Path) -> Option<PathBuf> {
    if !project.join("Cargo.toml").is_file() {
        return None;
    }
    let root = workspace_root(project);
    let root = fs::canonicalize(&root).unwrap_or(root);
    let project = fs::canonicalize(project).unwrap_or_else(|_| project.to_path_buf());
    project
        .strip_prefix(&root)
        .ok()
        .filter(|member| !member.as_os_str().is_empty())
        .map(Path::to_path_buf)
}

/// The directory of the package named `name` in the workspace that holds
/// `start`, as `cargo run -p <name>` finds it: the workspace's root package
/// or one of its `members`. A member pattern may end in `*`, as in
/// `"examples/*"`.
#[must_use]
pub fn package(start: &Path, name: &str) -> Option<PathBuf> {
    let manifest = |directory: &Path| {
        fs::read_to_string(directory.join("Cargo.toml"))
            .ok()
            .and_then(|text| text.parse::<DocumentMut>().ok())
    };
    let named = |directory: &Path| {
        manifest(directory).is_some_and(|document| {
            document
                .get("package")
                .and_then(|package| package.get("name"))
                .and_then(|name| name.as_str())
                == Some(name)
        })
    };
    let root = start
        .ancestors()
        .find(|directory| directory.join("Cargo.toml").is_file())
        .map(workspace_root)?;
    if named(&root) {
        return Some(root);
    }
    let document = manifest(&root)?;
    let members = document.get("workspace")?.get("members")?.as_array()?;
    members
        .iter()
        .filter_map(|member| member.as_str())
        .flat_map(|member| match member.strip_suffix('*') {
            Some(prefix) => fs::read_dir(root.join(prefix))
                .into_iter()
                .flatten()
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .collect(),
            None => vec![root.join(member)],
        })
        .find(|directory| named(directory))
}

/// The project itself when its manifest declares `[workspace]` or none
/// encloses it, its `package.workspace` when set, else the nearest enclosing
/// manifest with `[workspace]`.
#[must_use]
pub fn workspace_root(project: &Path) -> PathBuf {
    let manifest = |directory: &Path| {
        fs::read_to_string(directory.join("Cargo.toml"))
            .ok()
            .and_then(|text| text.parse::<DocumentMut>().ok())
    };
    if let Some(own) = manifest(project) {
        if own.contains_key("workspace") {
            return project.to_path_buf();
        }
        if let Some(path) = own
            .get("package")
            .and_then(|package| package.get("workspace"))
            .and_then(|workspace| workspace.as_str())
        {
            return project.join(path);
        }
    }
    project
        .ancestors()
        .skip(1)
        .find(|directory| {
            manifest(directory).is_some_and(|parent| parent.contains_key("workspace"))
        })
        .unwrap_or(project)
        .to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::{
        build_directory, member_path, package, rawr_directory, resolved, shared_path,
        target_directory,
    };
    use std::fs;
    use std::path::Path;

    fn fixture(label: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("rawr-location-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::canonicalize(root).unwrap()
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn rawr_products_follow_the_cargo_target_directory() {
        let root = fixture("target");
        let project = root.join("workspace/app");
        write(
            &root.join("workspace/Cargo.toml"),
            "[workspace]\nmembers = [\"app\"]\n",
        );
        write(&project.join("Cargo.toml"), "[package]\nname = \"app\"\n");
        let none = |_: &str| None;

        // A workspace member builds into the workspace's target.
        assert_eq!(
            target_directory(&project, none, None),
            root.join("workspace/target")
        );
        // Its own `[workspace]` makes the project the root.
        write(
            &project.join("Cargo.toml"),
            "[package]\nname = \"app\"\n[workspace]\n",
        );
        assert_eq!(
            target_directory(&project, none, None),
            project.join("target")
        );
        // Cargo config: the deepest file wins, relative to its `.cargo` parent.
        write(
            &root.join(".cargo/config.toml"),
            "[build]\ntarget-dir = \"outer\"\n",
        );
        assert_eq!(target_directory(&project, none, None), root.join("outer"));
        write(
            &project.join(".cargo/config.toml"),
            "[build]\ntarget-dir = \"inner\"\n",
        );
        assert_eq!(
            target_directory(&project, none, None),
            project.join("inner")
        );
        // The environment overrides every config file.
        let environment = |key: &str| (key == "CARGO_TARGET_DIR").then_some("/elsewhere");
        assert_eq!(
            target_directory(&project, environment, None),
            Path::new("/elsewhere")
        );
        fs::remove_dir_all(root).unwrap();
    }

    /// #1934: two projects that name one target directory each keep their
    /// own products there, so neither builds from the other's crate.
    #[test]
    fn projects_sharing_a_target_directory_keep_their_products_apart() {
        let root = fixture("shared");
        write(
            &root.join(".cargo/config.toml"),
            "[build]\ntarget-dir = \"_target\"\n",
        );
        for name in ["one", "two"] {
            write(
                &root.join(name).join("Cargo.toml"),
                &format!("[package]\nname = \"{name}\"\n[workspace]\n"),
            );
        }
        let none = |_: &str| None;
        let one = rawr_directory(&root.join("one"), none, None);
        let two = rawr_directory(&root.join("two"), none, None);
        assert_ne!(one, two);
        for (directory, name) in [(&one, "one"), (&two, "two")] {
            let keyed = directory
                .strip_prefix(root.join("_target/rawr/projects"))
                .unwrap();
            assert!(keyed.to_string_lossy().starts_with(&format!("{name}-")));
        }
        // The same project spelled through `..` keeps the same directory.
        assert_eq!(rawr_directory(&root.join("two/../one"), none, None), one);
        // #1984: each names its directory apart from the other's.
        let shared = |directory: &Path, name: &str| {
            shared_path(directory, &root.join(name))
                .map(|path| root.join("_target/rawr").join(path))
        };
        assert_eq!(shared(&one, "one").as_ref(), Some(&one));
        assert_eq!(shared(&two, "two").as_ref(), Some(&two));
        // A project's own `target` stays as it was.
        let environment = |key: &str| (key == "CARGO_TARGET_DIR").then_some("target");
        let own = rawr_directory(&root.join("one"), environment, None);
        assert_eq!(own, root.join("one/target/rawr"));
        assert_eq!(shared_path(&own, &root.join("one")), None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_workspace_member_keeps_its_products_under_the_workspace_target() {
        let root = fixture("member");
        write(
            &root.join("Cargo.toml"),
            "[package]\nname = \"top\"\n[workspace]\nmembers = [\"examples/*\", \"tools/one\"]\n",
        );
        write(
            &root.join("examples/game/Cargo.toml"),
            "[package]\nname = \"game\"\n",
        );
        write(
            &root.join("tools/one/Cargo.toml"),
            "[package]\nname = \"one\"\n",
        );
        let none = |_: &str| None;
        assert_eq!(rawr_directory(&root, none, None), root.join("target/rawr"));
        assert_eq!(
            rawr_directory(&root.join("examples/game"), none, None),
            root.join("target/rawr/members/examples/game")
        );
        assert_eq!(
            shared_path(
                &root.join("target/rawr/members/examples/game"),
                &root.join("examples/game")
            ),
            Some(Path::new("examples/game").to_path_buf())
        );
        assert_eq!(member_path(&root), None);
        assert_eq!(
            member_path(&root.join("tools/one")),
            Some(Path::new("tools/one").to_path_buf())
        );
        // `-p <name>` from anywhere in the workspace.
        assert_eq!(package(&root, "game"), Some(root.join("examples/game")));
        assert_eq!(package(&root.join("tools/one"), "top"), Some(root.clone()));
        assert_eq!(
            package(&root.join("examples/game/src"), "one"),
            Some(root.join("tools/one"))
        );
        assert_eq!(package(&root, "missing"), None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_build_directory_is_the_target_directory_unless_configured() {
        let root = fixture("build");
        let project = root.join("app");
        write(
            &project.join("Cargo.toml"),
            "[package]\nname = \"app\"\n[workspace]\n",
        );
        let none = |_: &str| None;
        assert_eq!(
            build_directory(&project, none, None),
            project.join("target")
        );
        write(
            &project.join(".cargo/config.toml"),
            "[build]\nbuild-dir = \"{workspace-root}/intermediate\"\n",
        );
        assert_eq!(
            build_directory(&project, none, None),
            project.join("intermediate")
        );
        let environment = |key: &str| (key == "CARGO_BUILD_BUILD_DIR").then_some("/elsewhere");
        assert_eq!(
            build_directory(&project, environment, None),
            Path::new("/elsewhere")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_relative_target_directory_names_the_paths_the_absolute_one_does() {
        let root = fixture("relative");
        let project = root.join("examples/app");
        write(
            &project.join("Cargo.toml"),
            "[package]\nname = \"app\"\n[workspace]\n",
        );
        let absolute = root.join("examples/_target");
        let spelled = absolute.to_str().unwrap();
        let relative = |key: &str| (key == "CARGO_TARGET_DIR").then_some("../_target");
        let named = |key: &str| (key == "CARGO_TARGET_DIR").then_some(spelled);
        assert_eq!(target_directory(&project, relative, None), absolute);
        assert_eq!(target_directory(&project, named, None), absolute);
        let current = |key: &str| (key == "CARGO_TARGET_DIR").then_some("./out/.");
        assert_eq!(
            target_directory(&project, current, None),
            project.join("out")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolved_leaves_each_parent_as_the_filesystem_does() {
        let root = fixture("resolved");
        fs::create_dir_all(root.join("real/inner")).unwrap();
        std::os::unix::fs::symlink(root.join("real/inner"), root.join("link")).unwrap();
        // Through a symlink, `..` reaches the target's parent, not the link's.
        assert_eq!(resolved(&root.join("link/../file")), root.join("real/file"));
        // A symlink the path only names keeps its spelling.
        assert_eq!(resolved(&root.join("link/./file")), root.join("link/file"));
        // An absent directory is left lexically, and the root floors `..`.
        assert_eq!(
            resolved(&root.join("absent/deeper/../../file")),
            root.join("file")
        );
        assert_eq!(resolved(Path::new("/../a")), Path::new("/a"));
        // A relative path keeps the `..` that leaves where it starts.
        assert_eq!(resolved(Path::new("../a/./b/../c")), Path::new("../a/c"));
        fs::remove_dir_all(root).unwrap();
    }
}
