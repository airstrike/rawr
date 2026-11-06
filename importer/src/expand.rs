//! A crate derive's expansion for a program's type (#593): rustc expands a
//! small crate of the derived item, built from the same cargo resolution
//! the metadata probe uses, and the produced items come back as source
//! text. Each expansion is computed once and kept in the project's
//! `target/rawr/expand/`, keyed by the item's text, its derives and the
//! artifacts they come from.

pub mod request;

use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

/// One program item to expand.
#[derive(Clone, Debug)]
pub struct Item {
    /// The item's text as the program writes it, its attributes first.
    pub text: String,
    /// Each derive the item names that a crate gives: the name it writes
    /// (`Serialize`, or `Load` after `use serde::Deserialize as Load;`) and
    /// the path it resolves to (`serde::Serialize`).
    pub derives: Vec<(String, String)>,
}

/// What an expansion needs of the probe's build: the target, where the
/// selected crates' own dependencies are, and each selected crate's
/// An alias, the artifact it names and that artifact's digest.
pub(crate) type Extern = (Box<str>, Box<str>, Box<str>);

/// artifact by the aliases the program names it by.
#[derive(Clone, Debug, Default)]
#[cfg_attr(
    not(feature = "rustc-driver"),
    allow(dead_code, reason = "only the driver's build runs an expansion")
)]
pub struct Basis {
    pub(crate) selected_target: Box<str>,
    pub(crate) dependency_directories: Box<[PathBuf]>,
    pub(crate) externs: Box<[Extern]>,
}

/// Where the expansions of the project at `project` are kept.
#[must_use]
pub fn directory(project: &Path) -> PathBuf {
    project.join("target/rawr/expand")
}

/// The small crate's source: one `use` for each derive, then the item.
/// Each derive's byte offset in it is where the driver reports its items.
#[must_use]
pub fn source(item: &Item) -> String {
    let mut source = String::new();
    for (written, path) in &item.derives {
        // A derive written as a path (`serde::Serialize`) needs no `use`.
        if !written.contains("::") {
            source.push_str(&format!(
                "#[allow(unused_imports)]\nuse {path} as {written};\n"
            ));
        }
    }
    source.push_str(&item.text);
    source.push('\n');
    source
}

/// The cache key of `item`'s expansion against `basis`, under `protocol`
/// and `edition`: the item's text, its derives, and the digests of the
/// artifacts its derives' crates are.
#[must_use]
pub fn key(item: &Item, basis: &Basis, protocol: u32, edition: &str) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"rawr.import.expand/v1");
    digest.update(protocol.to_le_bytes());
    digest.update(edition.as_bytes());
    digest.update([0]);
    digest.update(source(item).as_bytes());
    for (_, path) in &item.derives {
        let root = path.split("::").next().unwrap_or_default();
        for (alias, _, artifact) in basis.externs.iter().filter(|(alias, ..)| **alias == *root) {
            digest.update(alias.as_bytes());
            digest.update([0]);
            digest.update(artifact.as_bytes());
            digest.update([0]);
        }
    }
    digest.finalize().into()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The expansion kept at `key` in `directory`, when there is one.
#[must_use]
pub fn cached(directory: &Path, key: &[u8; 32], protocol: u32) -> Option<request::Expanded> {
    let bytes = std::fs::read(directory.join(format!("{}.json", hex(key)))).ok()?;
    let expanded: request::Expanded = serde_json::from_slice(&bytes).ok()?;
    (expanded.protocol == protocol).then_some(expanded)
}

#[cfg(feature = "rustc-driver")]
impl Basis {
    pub(crate) fn of(expansion: &crate::extract::plan::Expansion) -> Self {
        Self {
            selected_target: expansion.selected_target.clone(),
            dependency_directories: expansion.dependency_directories.clone(),
            externs: expansion.externs.clone(),
        }
    }

    /// The basis of `request`'s resolution, which builds the project's
    /// dependencies with the pinned nightly as the metadata probe does.
    pub fn capture(
        request: crate::cargo::Request,
        native: &crate::process::Native,
        limits: &crate::cargo::Limits,
    ) -> crate::Result<Self> {
        let resolution = request.resolve(native, limits)?;
        resolution
            .expansion()
            .map(Self::of)
            .ok_or_else(|| crate::Error::Request {
                reason: "the resolution built no crate a derive may come from".into(),
            })
    }
}

/// `item`'s expansion: the one kept in the project's `directory`, else
/// rustc's through `native`'s driver, which is then kept.
#[cfg(feature = "rustc-driver")]
pub fn expand(
    item: &Item,
    basis: &Basis,
    edition: &str,
    directory: &Path,
    native: &crate::process::Native,
    limits: &crate::cargo::Limits,
) -> crate::Result<request::Expanded> {
    use crate::process::{Command, Program, ToolPaths as _};
    let protocol = crate::extract::protocol::SCHEMA;
    let key = key(item, basis, protocol, edition);
    if let Some(expanded) = cached(directory, &key, protocol) {
        return Ok(expanded);
    }
    let io = |operation: &'static str, path: &Path| {
        let path = path.to_path_buf();
        move |source| crate::Error::Io {
            operation,
            path,
            source,
        }
    };
    let name = hex(&key);
    let work = directory.join(format!(".{name}.{}", std::process::id()));
    std::fs::create_dir_all(&work).map_err(io("create the expansion directory", &work))?;
    let source_path = work.join("lib.rs");
    std::fs::write(&source_path, source(item))
        .map_err(io("write the expansion source", &source_path))?;
    let roots = item
        .derives
        .iter()
        .map(|(_, path)| path.split("::").next().unwrap_or_default())
        .collect::<std::collections::BTreeSet<_>>();
    let request = request::Request {
        protocol,
        edition: edition.to_owned(),
        selected_target: basis.selected_target.to_string(),
        dependency_directories: basis
            .dependency_directories
            .iter()
            .map(|directory| directory.display().to_string())
            .collect(),
        externs: basis
            .externs
            .iter()
            .filter(|(alias, ..)| roots.contains(alias.as_ref()))
            .map(|(alias, path, _)| request::Extern {
                alias: alias.to_string(),
                path: path.to_string(),
            })
            .collect(),
        source: source_path.display().to_string(),
    };
    let request_path = work.join("request.json");
    let bytes = serde_json::to_vec(&request).map_err(|error| crate::Error::Request {
        reason: error.to_string().into(),
    })?;
    std::fs::write(&request_path, bytes)
        .map_err(io("write the expansion request", &request_path))?;
    let out = work.join("expanded.json");
    let production = native.production();
    let command = Command::expand(
        vec![
            "--expand".into(),
            production.rustc().as_os_str().to_owned(),
            request_path.as_os_str().to_owned(),
            out.as_os_str().to_owned(),
        ],
        work.clone(),
        crate::process::driver_library(production.rustc(), &[])
            .into_iter()
            .collect(),
    );
    let ran = crate::process::Launcher::run(&production, &command, limits.process_limits());
    // std's own derives (`Debug`, `Clone`) expand in the small crate too;
    // only the crates' derives the item names are its expansion here.
    let named = item
        .derives
        .iter()
        .filter_map(|(_, path)| path.rsplit("::").next())
        .collect::<std::collections::BTreeSet<_>>();
    let expanded = ran
        .and_then(|output| output.checked(Program::Driver))
        .and_then(|_| {
            let bytes = std::fs::read(&out).map_err(io("read the expansion", &out))?;
            let mut expanded =
                serde_json::from_slice::<request::Expanded>(&bytes).map_err(|error| {
                    crate::Error::Request {
                        reason: format!("the driver's expansion: {error}").into(),
                    }
                })?;
            expanded
                .expansions
                .retain(|expansion| named.contains(expansion.derive.as_str()));
            Ok(expanded)
        });
    let kept = directory.join(format!("{name}.json"));
    if let Ok(expanded) = &expanded
        && let Ok(bytes) = serde_json::to_vec(expanded)
    {
        let staged = work.join("kept.json");
        if std::fs::write(&staged, bytes).is_ok() {
            let _ = std::fs::rename(&staged, &kept);
        }
    }
    let _ = std::fs::remove_dir_all(&work);
    expanded
}

#[cfg(test)]
mod tests {
    use super::{key, source, Basis, Item};

    fn task() -> Item {
        Item {
            text: "#[derive(Debug, Serialize)]\nstruct Task { done: bool }".to_owned(),
            derives: vec![("Serialize".to_owned(), "serde::Serialize".to_owned())],
        }
    }

    fn basis(digest: &str) -> Basis {
        Basis {
            selected_target: "aarch64-apple-darwin".into(),
            dependency_directories: Box::default(),
            externs: Box::new([("serde".into(), "/deps/libserde.rmeta".into(), digest.into())]),
        }
    }

    #[test]
    fn the_source_names_each_written_derive_then_the_item() {
        assert_eq!(
            source(&task()),
            "#[allow(unused_imports)]\nuse serde::Serialize as Serialize;\n\
             #[derive(Debug, Serialize)]\nstruct Task { done: bool }\n"
        );
        let pathed = Item {
            text: "#[derive(serde::Serialize)]\nstruct Task;".to_owned(),
            derives: vec![("serde::Serialize".to_owned(), "serde::Serialize".to_owned())],
        };
        assert_eq!(
            source(&pathed),
            "#[derive(serde::Serialize)]\nstruct Task;\n"
        );
    }

    #[test]
    fn the_key_follows_the_item_and_its_derives_artifacts_only() {
        let item = task();
        let base = key(&item, &basis("a"), 51, "2024");
        assert_eq!(base, key(&item, &basis("a"), 51, "2024"));
        assert_ne!(base, key(&item, &basis("b"), 51, "2024"));
        assert_ne!(base, key(&item, &basis("a"), 52, "2024"));
        assert_ne!(base, key(&item, &basis("a"), 51, "2021"));
        let renamed = Item {
            text: item.text.replace("done", "finished"),
            ..item.clone()
        };
        assert_ne!(base, key(&renamed, &basis("a"), 51, "2024"));
        // Another crate's artifact is not this item's input.
        let mut other = basis("a");
        other.externs = Box::new([
            ("serde".into(), "/deps/libserde.rmeta".into(), "a".into()),
            ("uuid".into(), "/deps/libuuid.rmeta".into(), "z".into()),
        ]);
        assert_eq!(base, key(&item, &other, 51, "2024"));
    }
}
