//! Where a project's Rawr products live.
//!
//! Cargo keeps a project's build output in the project's target directory, and
//! so does Rawr: `<target>/rawr` holds what Rawr builds and keeps for a
//! project. `cargo clean` removes it.

use std::fs;
use std::path::{Path, PathBuf};

use toml_edit::DocumentMut;

/// The project's Cargo target directory: `CARGO_TARGET_DIR`, then
/// `CARGO_BUILD_TARGET_DIR`, then `build.target-dir` from the Cargo config files
/// Cargo reads for `project` (the deepest `.cargo/config.toml` first, then
/// `$CARGO_HOME/config.toml`), else `target` under the workspace root.
pub fn target_directory<'a>(
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
        (!value.contains('{')).then(|| base.join(value))
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

/// The project itself when its manifest declares `[workspace]` or none
/// encloses it, its `package.workspace` when set, else the nearest enclosing
/// manifest with `[workspace]`.
fn workspace_root(project: &Path) -> PathBuf {
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
    use super::{build_directory, target_directory};
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
}
