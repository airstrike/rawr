//! How a project that imports Rust builds. Rawr compiles the project's
//! dependencies in the project's own build directory with the importer's
//! pinned nightly, for the host named as a target, so `cargo build` and
//! `rawr run` share every unit. A project gets the two files below when it
//! first needs the importer: a plain marked crate needs neither.

use std::path::{Path, PathBuf};

use rawr_import::cargo::config::Environment;

/// The `rust-toolchain.toml` a project that imports Rust needs: the
/// importer's pinned nightly, the compiler Rawr builds the project's runner
/// and reads its dependencies with, so `cargo build` uses it too.
#[must_use]
pub fn toolchain() -> String {
    format!(
        "[toolchain]\nchannel = \"{}\"\ncomponents = [\"rust-src\", \"rustfmt\", \"clippy\"]\n",
        rawr_import::cargo::toolchain::CHANNEL
    )
}

/// The `.cargo/config.toml` a project that imports Rust needs:
/// `build.target` names the host (Cargo's `host-tuple`, so the file holds on
/// any machine), as Rawr's builds do, so `cargo build` and `rawr run`
/// compile the same units into `target/<host>/debug` and each reuses the
/// other's.
#[must_use]
pub fn cargo_config() -> String {
    "[build]\ntarget = \"host-tuple\"\n".to_owned()
}

/// Whether the project at `root` builds as Rawr builds it. `Err` names what
/// it needs, as the files to add. Otherwise every crate would be compiled
/// twice.
///
/// A member of a workspace rooted elsewhere is not asked: `cargo build`
/// takes its toolchain and config from the workspace root's directory, and
/// pinning the importer's nightly there would pin it for every crate in the
/// workspace. Rawr builds the member with its own toolchain into the
/// workspace's target directory, and `cargo build` compiles the member's
/// dependencies separately.
pub fn same_build(root: &Path) -> Result<(), String> {
    if rawr_import::location::member_path(root).is_some() {
        return Ok(());
    }
    let environment = Environment::capture().map_err(|error| error.to_string())?;
    // rustup decides the toolchain from the directory: `RUSTUP_TOOLCHAIN`,
    // an override, a `rust-toolchain.toml` above it, or the default.
    let active = std::process::Command::new("rustup")
        .args(["show", "active-toolchain"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_owned()
        });
    let target = rawr_import::location::build_target(root, &environment);
    let overrides = rawr_import::location::rawr_profile_configs(root, &environment);
    same_build_as(root, active.as_deref(), target.as_deref(), &overrides)
}

/// [`same_build`] for a project whose toolchain is `active` (`None` when
/// rustup cannot tell), whose `cargo build` target is `target`, and whose
/// Cargo config files `overrides` set a profile for the `rawr` crate.
fn same_build_as(
    root: &Path,
    active: Option<&str>,
    target: Option<&str>,
    overrides: &[PathBuf],
) -> Result<(), String> {
    let channel = rawr_import::cargo::toolchain::CHANNEL;
    let host = rawr_import::cargo::toolchain::HOST.trim();
    let mut needed = Vec::new();
    if let Some(active) = active
        && active != channel
        && !active.starts_with(&format!("{channel}-"))
    {
        needed.push(format!(
            "It builds with `{active}`, so add `rust-toolchain.toml`:\n\n{}",
            toolchain()
        ));
    }
    if !matches!(target, Some(target) if target == host || target == "host-tuple") {
        needed.push(format!(
            "It builds for {}, so add `.cargo/config.toml`:\n\n{}",
            target.map_or_else(
                || "no named target".to_owned(),
                |target| format!("`{target}`")
            ),
            cargo_config()
        ));
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
        "`{}` must build as Rawr builds it, with `{channel}` for `{host}`, or each of its \
         crates is compiled twice: once by `cargo build` and once by Rawr. {}",
        root.display(),
        needed.join("\n")
    ))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    #[test]
    fn a_project_builds_as_rawr_builds_it_or_is_told_what_to_add() {
        let root = Path::new("/project");
        let channel = rawr_import::cargo::toolchain::CHANNEL;
        let host = rawr_import::cargo::toolchain::HOST.trim();
        let pinned = format!("{channel}-{host}");
        assert_eq!(
            super::same_build_as(root, Some(&pinned), Some(host), &[]),
            Ok(())
        );
        assert_eq!(super::same_build_as(root, None, Some(host), &[]), Ok(()));
        assert_eq!(
            super::same_build_as(root, None, Some("host-tuple"), &[]),
            Ok(())
        );
        let error = super::same_build_as(root, Some(&pinned), None, &[]).unwrap_err();
        assert!(error.contains(".cargo/config.toml"), "{error}");
        assert!(!error.contains("rust-toolchain.toml"), "{error}");
        let error =
            super::same_build_as(root, Some("stable-aarch64-apple-darwin"), Some(host), &[])
                .unwrap_err();
        assert!(error.contains("rust-toolchain.toml"), "{error}");
        assert!(error.contains(channel), "{error}");
        let config = PathBuf::from("/checkout/.cargo/config.toml");
        let error = super::same_build_as(root, Some(&pinned), Some(host), &[config]).unwrap_err();
        assert!(error.contains("/checkout/.cargo/config.toml"), "{error}");
    }
}
