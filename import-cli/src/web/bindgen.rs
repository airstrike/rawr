//! wasm-bindgen's CLI, which turns a runner built for wasm32 into the
//! module and the JavaScript glue a page imports. Its version must be the
//! one of the `wasm-bindgen` crate the runner links, which the module names
//! in its section for the CLI: the one on the `PATH` serves when it is that
//! version, and otherwise rawr installs that version once in its home.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::Error;

/// The CLI's executable.
const EXECUTABLE: &str = "wasm-bindgen";

/// The custom section wasm-bindgen's macros leave in a module for its CLI.
const SECTION: &[u8] = b"__wasm_bindgen_unstable";

/// wasm-bindgen's CLI at the version the module `runner` links, installed
/// under `home`, rawr's home, if no other serves. `scratch` is the
/// directory its build may use, removed once it is installed.
pub(super) fn tool(runner: &Path, home: &Path, scratch: &Path) -> Result<PathBuf, Error> {
    let module = std::fs::read(runner).map_err(|source| Error::File {
        path: runner.to_path_buf(),
        source,
    })?;
    let version = linked(&module).ok_or_else(|| Error::Unlinked(runner.to_path_buf()))?;
    let version = version.as_str();
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

/// The version of wasm-bindgen `module` links: the `version` of the header
/// its section for the CLI starts with, as the CLI reads it. Each crate
/// that exports through wasm-bindgen adds a part to the section, and each
/// part's header names the one version the module links.
fn linked(module: &[u8]) -> Option<String> {
    let mut rest = module.strip_prefix(b"\0asm")?.get(4..)?;
    while let Some((&id, after)) = rest.split_first() {
        let (size, after) = leb128(after)?;
        let section = after.get(..size)?;
        rest = after.get(size..)?;
        if id != 0 {
            continue;
        }
        let (length, named) = leb128(section)?;
        if named.get(..length)? != SECTION {
            continue;
        }
        let part = named.get(length..)?;
        let header = u32::from_le_bytes(part.get(..4)?.try_into().ok()?);
        let header = part.get(4..4 + usize::try_from(header).ok()?)?;
        let header: serde_json::Value = serde_json::from_slice(header).ok()?;
        return header["version"].as_str().map(str::to_owned);
    }
    None
}

/// The unsigned LEB128 number `bytes` start with, and the bytes after it.
fn leb128(bytes: &[u8]) -> Option<(usize, &[u8])> {
    let mut value = 0usize;
    for (index, byte) in bytes.iter().enumerate().take(5) {
        value |= usize::from(byte & 0x7f) << (7 * index);
        if byte & 0x80 == 0 {
            return Some((value, &bytes[index + 1..]));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    /// A module with a type section, then a custom section `name` that
    /// holds `payload`.
    fn module(name: &[u8], payload: &[u8]) -> Vec<u8> {
        let mut section = vec![u8::try_from(name.len()).expect("a short name")];
        section.extend_from_slice(name);
        section.extend_from_slice(payload);
        let mut module = b"\0asm\x01\0\0\0".to_vec();
        module.extend_from_slice(&[1, 1, 0]);
        module.push(0);
        module.push(u8::try_from(section.len()).expect("a short section"));
        module.extend_from_slice(&section);
        module
    }

    /// The version is the one the header of the module's section for the
    /// CLI names, whatever follows it.
    #[test]
    fn the_module_names_the_linked_version() {
        let header = br#"{"schema_version":"0.2.130","version":"0.2.130"}"#;
        let length = u32::try_from(header.len()).expect("a short header");
        let mut payload = length.to_le_bytes().to_vec();
        payload.extend_from_slice(header);
        payload.extend_from_slice(b"\x2a\x02\0\0more");
        let linked = super::linked(&module(super::SECTION, &payload));
        assert_eq!(linked.as_deref(), Some("0.2.130"));
        assert_eq!(super::linked(&module(b"name", &payload)), None);
        assert_eq!(super::linked(b"not a module"), None);
    }
}
