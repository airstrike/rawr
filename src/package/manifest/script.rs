//! A cargo script's manifest: its frontmatter block, expanded as cargo
//! expands it (D608 point 6).
//!
//! A script is its own package wherever it sits. Its expanded manifest and
//! its lockfile live in a directory of rawr's area for scripts, keyed by the
//! script's path ([`directory`]), never beside the script. Every script
//! builds in one build directory there, so cargo compiles a crate once for
//! every script that uses it.

use std::path::{Path, PathBuf};

use toml_edit::{Document, DocumentMut, Item, Table, TableLike, Value};

use super::cargo::Cargo;
use super::{byte_span, Error};
use crate::project::frontmatter;

/// The edition cargo gives a script that names none: the latest.
const EDITION: &str = "2024";

/// What cargo refuses in a script's manifest, as cargo names it.
const REFUSED: [&str; 8] = [
    "workspace",
    "lib",
    "bin",
    "example",
    "test",
    "bench",
    "build-dependencies",
    "build_dependencies",
];

/// What cargo refuses in a script's `[package]`.
const REFUSED_PACKAGE: [&str; 10] = [
    "workspace",
    "build",
    "metabuild",
    "links",
    "autolib",
    "autobins",
    "autoexamples",
    "autotests",
    "autobenches",
    "default-run",
];

/// Rawr's area for scripts, in rawr's home.
#[must_use]
pub fn area() -> PathBuf {
    home().join("scripts")
}

/// Rawr's home: `$RAWR_HOME`, else `$CARGO_HOME/rawr`, else `~/.cargo/rawr`.
fn home() -> PathBuf {
    let cargo_home = std::env::var_os("CARGO_HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".cargo")))
        .unwrap_or_else(|| PathBuf::from(".cargo"));
    #[cfg(feature = "rust-import")]
    {
        rawr_import::store::home(&cargo_home)
    }
    #[cfg(not(feature = "rust-import"))]
    {
        std::env::var_os("RAWR_HOME")
            .filter(|home| !home.is_empty())
            .map_or_else(|| cargo_home.join("rawr"), PathBuf::from)
    }
}

/// The directory of the script at the canonical path `script` in rawr's
/// area: its expanded manifest, its lockfile and its target directory. It is
/// named by the script's file stem and a digest of its path, so a script
/// keeps its lockfile across runs and two scripts never share one.
#[must_use]
pub fn directory(script: &Path) -> PathBuf {
    directory_in(&area(), script)
}

fn directory_in(area: &Path, script: &Path) -> PathBuf {
    use sha2::{Digest as _, Sha256};
    let digest = Sha256::digest(script.as_os_str().as_encoded_bytes());
    let key = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    area.join(format!("{}-{key}", stem_name(script)))
}

/// The package name cargo gives a script without one: its file stem, with
/// what a package name can't hold replaced as cargo replaces it.
fn stem_name(script: &Path) -> String {
    let stem = script
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let placeholder = if stem.contains('_') { '_' } else { '-' };
    let mut name = String::with_capacity(stem.len());
    for character in stem.chars() {
        if name.is_empty() {
            if character == '_' || (character.is_alphabetic() && !character.is_ascii_digit()) {
                name.push(character);
            }
        } else if character.is_alphanumeric() || character == '-' || character == '_' {
            name.push(character);
        } else {
            name.push(placeholder);
        }
    }
    if name.is_empty() {
        name.push_str("package");
    }
    name
}

/// The manifest of the script `text` at the canonical path `script`, as
/// cargo expands its frontmatter, in rawr's area: `None` for a file with no
/// frontmatter block, or one cargo refuses, which the parse reports.
///
/// Errors point into the script. Relative paths in the manifest are the
/// script directory's, as cargo reads them.
pub(crate) fn cargo(script: &Path, text: &str) -> Result<Option<Cargo>, Error> {
    let Ok(frontmatter::Lead {
        frontmatter: Some(frontmatter),
        ..
    }) = frontmatter::read(text)
    else {
        return Ok(None);
    };
    let range = frontmatter.manifest.clone();
    // The manifest at its own bytes of the script, with what comes before
    // it blanked, so every span is the script's.
    let mut padded = text.as_bytes()[..range.start]
        .iter()
        .map(|byte| if *byte == b'\n' { '\n' } else { ' ' })
        .collect::<String>();
    padded.push_str(&text[range.clone()]);
    let span = |range: Option<std::ops::Range<usize>>| {
        byte_span(&padded, range.unwrap_or(frontmatter.open.clone()))
    };
    let document = Document::parse(padded.as_str()).map_err(|error| Error {
        message: error.message().into(),
        span: span(error.span()),
    })?;
    let table = document.as_table();
    let package = table.get("package").and_then(Item::as_table_like);
    let refused = REFUSED
        .iter()
        .filter_map(|key| table.get_key_value(key))
        .map(|(key, item)| (key.get().to_owned(), key.span().or_else(|| item.span())))
        .chain(package.into_iter().flat_map(|package| {
            REFUSED_PACKAGE
                .iter()
                .filter_map(|key| package.get_key_value(key))
                .map(|(key, item)| {
                    (
                        format!("package.{}", key.get()),
                        key.span().or_else(|| item.span()),
                    )
                })
        }))
        .collect::<Vec<_>>();
    if let Some((_, first)) = refused.first() {
        let names = refused
            .iter()
            .map(|(name, _)| format!("`{name}`"))
            .collect::<Vec<_>>();
        let verb = if names.len() == 1 { "is" } else { "are" };
        return Err(Error {
            message: format!(
                "{} {verb} not allowed in embedded manifests",
                names.join(", ")
            )
            .into(),
            span: span(first.clone()),
        });
    }
    let string = |key: &str| {
        package
            .and_then(|package| package.get(key))
            .and_then(Item::as_str)
            .map(str::to_owned)
    };
    let name = string("name").unwrap_or_else(|| stem_name(script));
    let edition = string("edition").unwrap_or_else(|| EDITION.to_owned());
    let base = script.parent().unwrap_or(Path::new("/"));
    let dependencies = super::cargo::dependencies(&padded, base, table, None)?;
    let expanded = expand(script, &text[range], &name, &edition).map_err(|message| Error {
        message: message.into(),
        span: span(None),
    })?;
    Ok(Some(Cargo {
        path: directory(script).join("Cargo.toml"),
        text: expanded,
        crate_name: name.replace('-', "_"),
        package: name,
        version: string("version"),
        edition: Some(edition),
        dependencies,
    }))
}

/// The frontmatter `manifest` of `script` as cargo expands it: its package
/// named and given an edition, one binary target that is the script, a
/// workspace of its own, and each relative path made the script
/// directory's.
fn expand(script: &Path, manifest: &str, name: &str, edition: &str) -> Result<String, String> {
    let mut document = manifest
        .parse::<DocumentMut>()
        .map_err(|error| error.to_string())?;
    let base = script.parent().unwrap_or(Path::new("/"));
    let package = document
        .entry("package")
        .or_insert_with(|| Item::Table(Table::new()))
        .as_table_like_mut()
        .ok_or("`package` must be a table")?;
    if !package.contains_key("name") {
        package.insert("name", toml_edit::value(name));
    }
    if !package.contains_key("edition") {
        package.insert("edition", toml_edit::value(edition));
    }
    let table = document.as_table_mut();
    absolute_dependencies(table, base);
    if let Some(targets) = table.get_mut("target").and_then(Item::as_table_like_mut) {
        for (_, target) in targets.iter_mut() {
            if let Some(target) = target.as_table_like_mut() {
                absolute_dependencies(target, base);
            }
        }
    }
    // `[patch.<registry>.<name>]` and `[replace."<spec>"]`.
    if let Some(patches) = table.get_mut("patch").and_then(Item::as_table_like_mut) {
        for (_, registry) in patches.iter_mut() {
            if let Some(registry) = registry.as_table_like_mut() {
                absolute_entries(registry, base);
            }
        }
    }
    if let Some(replace) = table.get_mut("replace").and_then(Item::as_table_like_mut) {
        absolute_entries(replace, base);
    }
    let mut bin = Table::new();
    bin.insert("name", toml_edit::value(name));
    bin.insert(
        "path",
        toml_edit::value(script.to_string_lossy().into_owned()),
    );
    let mut bins = toml_edit::ArrayOfTables::new();
    bins.push(bin);
    table.insert("bin", Item::ArrayOfTables(bins));
    table.insert("workspace", Item::Table(Table::new()));
    Ok(format!(
        "# Generated by Rawr from {:?}: its frontmatter, as cargo reads a script.\n{document}",
        script.to_string_lossy()
    ))
}

/// Make each relative `path` of the dependency tables in `table` the
/// script directory's.
fn absolute_dependencies(table: &mut dyn TableLike, base: &Path) {
    for key in ["dependencies", "dev-dependencies", "dev_dependencies"] {
        if let Some(dependencies) = table.get_mut(key).and_then(Item::as_table_like_mut) {
            absolute_entries(dependencies, base);
        }
    }
}

/// Make the relative `path` of each entry of `entries` the script
/// directory's.
fn absolute_entries(entries: &mut dyn TableLike, base: &Path) {
    for (_, entry) in entries.iter_mut() {
        let Some(entry) = entry.as_table_like_mut() else {
            continue;
        };
        let Some(path) = entry.get("path").and_then(Item::as_str) else {
            continue;
        };
        if Path::new(path).is_absolute() {
            continue;
        }
        let absolute = base.join(path).to_string_lossy().into_owned();
        entry.insert("path", Item::Value(Value::from(absolute)));
    }
}

/// Write the script's expanded manifest to its directory in rawr's area
/// when it changed, with the area's Cargo config, and lock what the manifest
/// declares: cargo locks a new dependency and keeps every locked one, as it
/// does for a script it runs. The lockfile stays in the script's directory.
#[cfg(feature = "rust-import")]
pub(crate) fn materialize(cargo: &Cargo) -> Result<(), String> {
    use std::fs;
    let io =
        |path: &Path, error: std::io::Error| format!("cannot write `{}`: {error}", path.display());
    let directory = cargo
        .path
        .parent()
        .ok_or("a script's manifest has no directory")?;
    let area = directory
        .parent()
        .ok_or("a script's directory has no area")?;
    // One build directory for every script, so a crate one script compiled
    // is fresh for the next (D608 point 6).
    let config = area.join(".cargo/config.toml");
    let text = "# Written by Rawr: scripts share one Cargo build directory.\n\
                [build]\nbuild-dir = \"build\"\n";
    let write = |path: &Path, text: &str| -> Result<bool, String> {
        if fs::read_to_string(path).ok().as_deref() == Some(text) {
            return Ok(false);
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| io(parent, error))?;
        }
        fs::write(path, text).map_err(|error| io(path, error))?;
        Ok(true)
    };
    write(&config, text)?;
    let changed = write(&cargo.path, &cargo.text)?;
    if !changed && directory.join("Cargo.lock").is_file() {
        return Ok(());
    }
    // The user's cargo, the rustup proxy on `PATH`, as for the runner.
    let output = std::process::Command::new("cargo")
        .args(["update", "--workspace", "--quiet", "--manifest-path"])
        .arg(&cargo.path)
        .current_dir(directory)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| format!("cannot run cargo to lock the script's dependencies: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "cargo could not lock the script's dependencies:\n{}",
        String::from_utf8_lossy(&output.stderr).trim_end()
    ))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{cargo, directory_in, stem_name};

    fn script(text: &str) -> super::Cargo {
        cargo(Path::new("/work/tools/digits.rs"), text)
            .expect(text)
            .expect(text)
    }

    #[test]
    fn a_script_s_manifest_is_its_frontmatter_as_cargo_expands_it() {
        let cargo = script(
            "#!/usr/bin/env rawr\n---cargo\n[dependencies]\nitoa = \"1\"\nlocal = { path = \"../local\" }\n---\n// rawr\nfn main() {}\n",
        );
        assert_eq!(cargo.package, "digits");
        assert_eq!(cargo.edition.as_deref(), Some("2024"));
        let aliases = cargo
            .dependencies
            .iter()
            .map(|dependency| dependency.alias.as_str())
            .collect::<Vec<_>>();
        assert_eq!(aliases, ["itoa", "local"]);
        assert!(cargo.path.ends_with("Cargo.toml"));
        assert!(
            cargo.path.parent().unwrap().starts_with(super::area()),
            "{}",
            cargo.path.display()
        );
        let manifest = cargo.text.parse::<toml_edit::DocumentMut>().unwrap();
        assert_eq!(manifest["package"]["name"].as_str(), Some("digits"));
        assert_eq!(manifest["package"]["edition"].as_str(), Some("2024"));
        assert_eq!(
            manifest["dependencies"]["local"]["path"].as_str(),
            Some("/work/tools/../local")
        );
        assert_eq!(manifest["dependencies"]["itoa"].as_str(), Some("1"));
        assert_eq!(
            manifest["bin"][0]["path"].as_str(),
            Some("/work/tools/digits.rs")
        );
        assert!(manifest["workspace"].is_table());

        let named = script(
            "---cargo\n[package]\nname = \"counter\"\nedition = \"2021\"\n---\nfn main() {}\n",
        );
        assert_eq!(named.package, "counter");
        assert_eq!(named.edition.as_deref(), Some("2021"));
        assert!(named.dependencies.is_empty());
        assert_eq!(
            super::cargo(Path::new("/x.rs"), "fn main() {}").unwrap(),
            None
        );
    }

    #[test]
    fn what_cargo_refuses_in_a_script_s_manifest_is_refused_at_its_line() {
        for (text, line, names) in [
            ("---cargo\n[workspace]\n---\n", 2, "`workspace` is"),
            (
                "---\n[package]\nbuild = \"build.rs\"\n[lib]\n---\n",
                4,
                "`lib`, `package.build` are",
            ),
            (
                "---cargo\n\n[build-dependencies]\ncc = \"1\"\n---\n",
                3,
                "`build-dependencies` is",
            ),
        ] {
            let error = cargo(Path::new("/x.rs"), text).expect_err(text);
            assert!(
                error.message.contains(names) && error.message.contains("embedded manifests"),
                "{}",
                error.message
            );
            assert_eq!(error.span.start().line(), Some(line), "{text:?}");
        }
        let error =
            cargo(Path::new("/x.rs"), "#!/x\n---cargo\n[package\n---\n").expect_err("bad TOML");
        assert_eq!(error.span.start().line(), Some(3));
    }

    #[test]
    fn a_script_s_directory_is_keyed_by_its_path() {
        let area = Path::new("/home/.cargo/rawr/scripts");
        let one = directory_in(area, Path::new("/a/digits.rs"));
        let two = directory_in(area, Path::new("/b/digits.rs"));
        assert_ne!(one, two);
        assert_eq!(one, directory_in(area, Path::new("/a/digits.rs")));
        assert!(one
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("digits-"));
        assert_eq!(stem_name(Path::new("/a/my tool.rs")), "my-tool");
        assert_eq!(stem_name(Path::new("/a/snake_case.rs")), "snake_case");
        assert_eq!(stem_name(Path::new("/a/1st.rs")), "st");
        assert_eq!(stem_name(Path::new("/a/123.rs")), "package");
    }
}
