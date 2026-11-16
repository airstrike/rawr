//! wasm-bindgen's CLI, which turns a runner built for wasm32 into the
//! module and the JavaScript glue a page imports. Its version must be the
//! one of the `wasm-bindgen` crate the runner links, which the runner's
//! `Cargo.lock` names: the one on the `PATH` serves when it is that
//! version, and otherwise rawr installs that version once in its home.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::Error;

/// The CLI's executable.
const EXECUTABLE: &str = "wasm-bindgen";

/// wasm-bindgen's CLI at the version `lock`, a runner's `Cargo.lock`,
/// links, installed under `home`, rawr's home, if no other serves.
/// `scratch` is the directory its build may use, removed once it is
/// installed.
pub(super) fn tool(lock: &Path, home: &Path, scratch: &Path) -> Result<PathBuf, Error> {
    let text = std::fs::read_to_string(lock).map_err(|source| Error::File {
        path: lock.to_path_buf(),
        source,
    })?;
    let version = linked(&text).ok_or_else(|| Error::Unlinked(lock.to_path_buf()))?;
    if is_version(Path::new(EXECUTABLE), version) {
        return Ok(PathBuf::from(EXECUTABLE));
    }
    let root = home.join("tools").join("wasm-bindgen").join(version);
    let installed = root.join("bin").join(EXECUTABLE);
    if is_version(&installed, version) {
        return Ok(installed);
    }
    rawr_import::process::say(format_args!(
        "rawr: installing wasm-bindgen-cli {version}, which packs the runner for a page"
    ));
    let status = Command::new("cargo")
        .args([
            "install",
            "wasm-bindgen-cli",
            "--locked",
            "--version",
            version,
            "--root",
        ])
        .arg(&root)
        .env("CARGO_TARGET_DIR", scratch)
        .env_remove("CARGO_BUILD_TARGET")
        .status()
        .map_err(|source| Error::Spawn {
            program: "cargo install".to_owned(),
            source,
        })?;
    let _ = std::fs::remove_dir_all(scratch);
    if status.success() && is_version(&installed, version) {
        Ok(installed)
    } else {
        Err(Error::Install(version.to_owned()))
    }
}

/// Whether `executable` is wasm-bindgen's CLI at `version`.
fn is_version(executable: &Path, version: &str) -> bool {
    Command::new(executable)
        .arg("--version")
        .stderr(Stdio::null())
        .output()
        .is_ok_and(|output| {
            String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .nth(1)
                == Some(version)
        })
}

/// The version of the `wasm-bindgen` package `lock` pins.
fn linked(lock: &str) -> Option<&str> {
    let mut lines = lock.lines();
    lines.find(|line| line.trim() == "name = \"wasm-bindgen\"")?;
    let line = lines.next()?.trim();
    line.strip_prefix("version = \"")?.strip_suffix('"')
}

#[cfg(test)]
mod tests {
    /// The version is the one of the package named `wasm-bindgen`, not of
    /// a package whose name starts with it.
    #[test]
    fn the_lock_names_the_linked_version() {
        let lock = "[[package]]\nname = \"wasm-bindgen-futures\"\nversion = \"0.4.55\"\n\n\
                    [[package]]\nname = \"wasm-bindgen\"\nversion = \"0.2.105\"\n";
        assert_eq!(super::linked(lock), Some("0.2.105"));
        assert_eq!(super::linked("[[package]]\nname = \"web-sys\"\n"), None);
    }
}
