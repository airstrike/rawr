//! How a project that imports Rust builds. The project's own Cargo builds
//! its runner, on the project's toolchain and with the project's config, in
//! the project's build directory, so `cargo build` and `rawr run` share every
//! unit (D609). A project needs no added files.

use std::path::{Path, PathBuf};

use rawr_import::cargo::config::Environment;

/// Whether the project at `root` builds as Rawr builds it. `Err` says what
/// to change.
///
/// The project's toolchain must be at least the `rawr` crate's
/// `rust-version`, since the runner links that crate. A Cargo config that
/// sets a profile for the `rawr` crate is refused, since Cargo cannot
/// combine it with the one Rawr's runner sets. A member of a workspace
/// rooted elsewhere takes its config from that workspace, so only its
/// toolchain is asked about.
pub fn same_build(root: &Path) -> Result<(), String> {
    let active = active_version(root);
    let required = required_version();
    let overrides = if rawr_import::location::member_path(root).is_some() {
        Vec::new()
    } else {
        let environment = Environment::capture().map_err(|error| error.to_string())?;
        rawr_import::location::rawr_profile_configs(root, &environment)
    };
    same_build_as(root, active.as_deref(), required.as_deref(), &overrides)
}

/// The version of the Rust compiler the project at `root` builds with, as
/// `rustc --version` prints it there: rustup picks the toolchain from the
/// directory (`RUSTUP_TOOLCHAIN`, an override, a `rust-toolchain.toml`
/// above it, or the default). `None` when it cannot tell.
fn active_version(root: &Path) -> Option<String> {
    let output = std::process::Command::new("rustc")
        .arg("--version")
        .current_dir(root)
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .nth(1)
        .map(str::to_owned)
}

/// The `rawr` crate's `rust-version`, from the manifest the runner builds
/// against. `None` when it has none.
fn required_version() -> Option<String> {
    let manifest = rawr_import::host::root().ok()?.join("Cargo.toml");
    rust_version(&std::fs::read_to_string(manifest).ok()?)
}

/// `[package] rust-version` in a manifest's text.
fn rust_version(manifest: &str) -> Option<String> {
    let mut package = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            package = line == "[package]";
        } else if package
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == "rust-version"
        {
            return Some(value.trim().trim_matches('"').to_owned());
        }
    }
    None
}

/// A version's numbers, without any pre-release part: `1.99.0-nightly`
/// counts as 1.99.0, as Cargo counts it against a `rust-version`.
fn numbers(version: &str) -> Vec<u64> {
    version
        .split(['-', '+'])
        .next()
        .unwrap_or_default()
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

/// [`same_build`] for a project whose compiler is `active` (`None` when it
/// cannot tell), against the `rawr` crate's `required` version, and whose
/// Cargo config files `overrides` set a profile for the `rawr` crate.
fn same_build_as(
    root: &Path,
    active: Option<&str>,
    required: Option<&str>,
    overrides: &[PathBuf],
) -> Result<(), String> {
    let mut needed = Vec::new();
    if let (Some(active), Some(required)) = (active, required) {
        let (mut have, mut want) = (numbers(active), numbers(required));
        let length = have.len().max(want.len());
        have.resize(length, 0);
        want.resize(length, 0);
        if have < want {
            needed.push(format!(
                "It builds with Rust {active}, and Rawr needs Rust {required} or newer. \
                 Update the toolchain with `rustup update`, or raise the one the project \
                 pins."
            ));
        }
    }
    for config in overrides {
        needed.push(format!(
            "`{}` sets a profile for the `rawr` crate, and Cargo cannot combine it with \
             the one Rawr's runner sets. A project inside a Rawr checkout inherits the \
             checkout's config; move the project out of it, or remove that profile.",
            config.display()
        ));
    }
    if needed.is_empty() {
        return Ok(());
    }
    Err(format!(
        "`{}` cannot build as Rawr builds it. {}",
        root.display(),
        needed.join("\n")
    ))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    #[test]
    fn a_project_builds_on_its_own_toolchain_from_rawrs_rust_version_up() {
        let root = Path::new("/project");
        for active in ["1.95.0", "1.96.1", "1.99.0-nightly", "2.0.0"] {
            assert_eq!(
                super::same_build_as(root, Some(active), Some("1.95.0"), &[]),
                Ok(()),
                "{active}"
            );
        }
        assert_eq!(
            super::same_build_as(root, None, Some("1.95.0"), &[]),
            Ok(())
        );
        assert_eq!(
            super::same_build_as(root, Some("1.80.0"), None, &[]),
            Ok(())
        );
        let error = super::same_build_as(root, Some("1.94.1"), Some("1.95.0"), &[]).unwrap_err();
        assert!(error.contains("1.94.1"), "{error}");
        assert!(error.contains("1.95.0"), "{error}");
        assert!(error.contains("rustup update"), "{error}");
        assert!(!error.contains("rust-toolchain.toml"), "{error}");
        assert!(!error.contains(".cargo/config.toml"), "{error}");
        let config = PathBuf::from("/checkout/.cargo/config.toml");
        let error =
            super::same_build_as(root, Some("1.95.0"), Some("1.95.0"), &[config]).unwrap_err();
        assert!(error.contains("/checkout/.cargo/config.toml"), "{error}");
    }

    #[test]
    fn the_rust_version_is_the_package_s() {
        assert_eq!(
            super::rust_version(
                "[workspace]\nrust-version = \"9\"\n\n[package]\nname = \"rawr\"\nrust-version = \"1.95.0\"\n"
            )
            .as_deref(),
            Some("1.95.0")
        );
        assert_eq!(super::rust_version("[package]\nname = \"rawr\"\n"), None);
        assert!(super::rust_version(include_str!("../../Cargo.toml")).is_some());
    }
}
