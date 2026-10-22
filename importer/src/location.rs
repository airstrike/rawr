//! Where a project's Rawr products live.
//!
//! Cargo keeps a project's build output in the project's target directory, and
//! so does Rawr: the typed store (extraction results, indexes, runner
//! executions, leases, pins, ledgers), the shared dependency build directory,
//! the generated runner sources and file digests live in `<target>/rawr`.
//! `cargo clean` removes them. Only what is keyed by the Rawr install alone
//! (its embedded host sources) stays under `$CARGO_HOME/rawr`.

use std::fs;
use std::path::{Path, PathBuf};

use toml_edit::DocumentMut;

/// `<target>/rawr` for `project`, the target directory resolved as Cargo does.
#[must_use]
pub fn rawr_directory(project: &Path, environment: &crate::cargo::config::Environment) -> PathBuf {
    let project = fs::canonicalize(project).unwrap_or_else(|_| project.to_path_buf());
    target_directory(
        &project,
        |key| environment.value_of(key),
        environment.cargo_home().ok().flatten().as_deref(),
    )
    .join("rawr")
}

/// The project's Cargo target directory: `CARGO_TARGET_DIR`, then
/// `CARGO_BUILD_TARGET_DIR`, then `build.target-dir` from the Cargo config files
/// Cargo reads for `project` (the deepest `.cargo/config.toml` first, then
/// `$CARGO_HOME/config.toml`), else `target` under the workspace root.
pub(crate) fn target_directory<'a>(
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

fn configured_target_dir(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let document = text.parse::<DocumentMut>().ok()?;
    document
        .get("build")?
        .get("target-dir")?
        .as_str()
        .map(str::to_owned)
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
    use super::target_directory;
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
}
