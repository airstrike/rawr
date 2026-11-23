//! A crate derive's expansion for a program's type (#593): rustc expands a
//! small crate of the derived items, built from the cargo resolution the
//! program's own run made, and the produced items come back as source text.
//! Each expansion is kept in the project's `target/rawr/expand/`.
//!
//! The source loading reads is kept at its item's path and derive, and names
//! the item text it was expanded from and the artifact of the crate its
//! derive comes from: an edit that changes only the expansion's bodies, as a
//! `#[serde(rename)]` does, changes that one source in place, as an edit to
//! a body changes its file (#1040), and a build that changes the crate
//! retires the source (`kept`, #2152).

pub mod call;
pub mod kept;
pub mod request;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

/// One program item to expand.
#[derive(Clone, Debug)]
pub struct Item {
    /// The item's path in its crate: its modules, then its name
    /// (`app::Task`).
    pub path: String,
    /// The item's text as the program writes it, its attributes first.
    pub text: String,
    /// Each derive the item names that a crate gives: the name it writes
    /// (`Serialize`, or `Load` after `use serde::Deserialize as Load;`) and
    /// the path it resolves to (`serde::Serialize`).
    pub derives: Vec<(String, String)>,
}

/// What an expansion needs of the run's build: the target, where the
/// selected crates' own dependencies are, and each selected crate's
/// artifact by the aliases the program names it by.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[cfg_attr(
    not(feature = "rustc-driver"),
    allow(dead_code, reason = "only the driver's build runs an expansion")
)]
pub struct Basis {
    pub(crate) selected_target: Box<str>,
    pub(crate) dependency_directories: Box<[PathBuf]>,
    /// Each alias, the artifact it names and that artifact's digest.
    pub(crate) externs: Box<[Extern]>,
}

/// One crate a derive may come from: the alias the program names it by,
/// its artifact, and that artifact's digest.
pub(crate) type Extern = (Box<str>, Box<str>, Box<str>);

/// Where the expansions of the project at `project` are kept.
#[must_use]
pub fn directory(project: &Path) -> PathBuf {
    project.join("target/rawr/expand")
}

/// Where the program at `project` keeps the expansion of the derive its
/// item at `path` (`app::Task`) writes as `written` (`Serialize`): a Rawr
/// source of its own, which loading the item's module reads beside it
/// (#593).
#[must_use]
pub fn source_path(project: &Path, path: &str, written: &str) -> PathBuf {
    let mut digest = Sha256::new();
    digest.update(b"rawr.import.expand.source/v2");
    digest.update(path.as_bytes());
    digest.update([0]);
    digest.update(written.as_bytes());
    let key: [u8; 32] = digest.finalize().into();
    directory(project).join(format!("{}.rs", hex(&key)))
}

/// The first line of an expansion source, before the macro's path.
const SOURCE_HEADER: &str = "// derive ";

/// The digest an expansion source names its item's text `text` by.
fn item_digest(text: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"rawr.import.expand.item/v2");
    digest.update(text.as_bytes());
    hex(&digest.finalize())
}

/// An expansion source's text: a line naming the macro `resolved`
/// (`serde::Serialize`) the items come from, the item text `item` they were
/// expanded from, the digest `artifact` of the macro crate's artifact they
/// were expanded against and the helper attributes the derive declares,
/// then the items `expanded` holds.
#[must_use]
pub fn source_text(
    resolved: &str,
    item: &str,
    artifact: &str,
    expanded: Option<&request::Expansion>,
) -> String {
    let (items, helpers) = expanded.map_or((&[][..], &[][..]), |expansion| {
        (expansion.items.as_slice(), expansion.helpers.as_slice())
    });
    let mut text = format!(
        "{SOURCE_HEADER}{resolved} {} {artifact} {}\n",
        item_digest(item),
        helpers.join(",")
    );
    for item in items {
        text.push_str(item);
        text.push('\n');
    }
    text
}

/// The helper attributes (`serde`) the derive a kept expansion source
/// `text` comes from declares, which its item and the item's members may
/// carry (#2516).
pub fn helpers(text: &str) -> impl Iterator<Item = &str> {
    let line = text.split_once('\n').map_or("", |(line, _)| line);
    line.strip_prefix(SOURCE_HEADER)
        .and_then(|words| words.split(' ').nth(3))
        .unwrap_or_default()
        .split(',')
        .filter(|helper| !helper.is_empty())
}

/// What a kept expansion source is of an item as the program writes it
/// now, each with the macro it comes from (`serde::Serialize`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kept<'a> {
    /// The expansion of the item's text: loading reads it.
    Current(&'a str),
    /// The expansion of the item's text before an edit: rustc expands the
    /// item again.
    Stale(&'a str),
}

/// What the expansion source `text` is of the item text `item`, as
/// [`source_text`] names its macro and item. `None` for a text that is no
/// expansion source.
#[must_use]
pub fn kept<'a>(text: &'a str, item: &str) -> Option<Kept<'a>> {
    let (line, _) = text.split_once('\n')?;
    let mut words = line.strip_prefix(SOURCE_HEADER)?.split(' ');
    let (resolved, digest) = (words.next()?, words.next()?);
    Some(if digest == item_digest(item) {
        Kept::Current(resolved)
    } else {
        Kept::Stale(resolved)
    })
}

/// The macro (`serde::Serialize`) a kept expansion source `text` comes
/// from, and the digest of the artifact it was expanded against. `None`
/// for a text that is no derive's expansion source.
#[cfg_attr(
    not(feature = "rustc-driver"),
    allow(dead_code, reason = "only the driver's build runs an expansion")
)]
fn built(text: &str) -> Option<(&str, &str)> {
    let (line, _) = text.split_once('\n')?;
    let mut words = line.strip_prefix(SOURCE_HEADER)?.split(' ');
    let resolved = words.next()?;
    let artifact = words.nth(1)?;
    Some((resolved, artifact))
}

/// Keep each of `item`'s derives' expansions in `expanded` as its source
/// under `project`, where loading the item's module reads it. A derive the
/// expansion has no items for gets an empty source: it expanded to nothing.
pub fn keep_sources(
    project: &Path,
    item: &Item,
    basis: &Basis,
    expanded: &request::Expanded,
) -> std::io::Result<()> {
    std::fs::create_dir_all(directory(project))?;
    let mut expansions = expanded.expansions.iter().collect::<Vec<_>>();
    for (written, resolved) in &item.derives {
        let name = macro_name(written);
        let expansion = expansions
            .iter()
            .position(|expansion| macro_name(&expansion.derive) == name)
            .map(|index| expansions.remove(index));
        let path = source_path(project, &item.path, written);
        let staged = path.with_extension(format!("rs.{}", std::process::id()));
        let artifact = basis.artifact(resolved).unwrap_or_default();
        std::fs::write(
            &staged,
            source_text(resolved, &item.text, artifact, expansion),
        )?;
        std::fs::rename(&staged, &path)?;
    }
    Ok(())
}

/// One item's part of the small crate: one `use` for each derive, then the
/// item.
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

#[cfg_attr(
    not(feature = "rustc-driver"),
    allow(dead_code, reason = "only the driver's build runs an expansion")
)]
/// The small crate of `items`: each item's [`source`] in a module of its
/// own, so two items' `use`s never meet, and the bytes each module spans,
/// in which the driver reports its derives' items.
fn crate_source(items: &[&Item]) -> (String, Vec<std::ops::Range<u32>>) {
    let mut text = String::new();
    let mut spans = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let start = text.len();
        text.push_str(&format!("mod __rawr_expand_{index} {{\n"));
        text.push_str(&source(item));
        text.push_str("}\n");
        let offset = |at: usize| u32::try_from(at).unwrap_or(u32::MAX);
        spans.push(offset(start)..offset(text.len()));
    }
    (text, spans)
}

/// A derive's name as rustc names its expansion, by the path written:
/// `Serialize` of `serde::Serialize`, and `Save` of a `Save` that a `use`
/// names `serde::Serialize` by.
fn macro_name(path: &str) -> &str {
    path.rsplit("::").next().unwrap_or(path)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Where a project's expansion directory keeps the basis its last
/// expansion ran against.
const BASIS: &str = "basis.json";

impl Basis {
    /// The digest of the artifact of the crate the macro at `path`
    /// (`serde::Serialize`) comes from, when this basis names it.
    #[must_use]
    pub fn artifact(&self, path: &str) -> Option<&str> {
        let root = path.split("::").next().unwrap_or_default();
        self.externs
            .iter()
            .find(|(alias, ..)| **alias == *root)
            .map(|(_, _, digest)| &**digest)
    }
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

    /// Whether an expansion against this basis can name the crate `alias`.
    #[must_use]
    pub fn names(&self, alias: &str) -> bool {
        self.externs.iter().any(|(name, ..)| **name == *alias)
    }

    /// The selected crates' artifacts, beside which each generated crate's
    /// check keeps its records (`crate::record`, `crate::uses`).
    pub fn artifacts(&self) -> impl Iterator<Item = &Path> {
        self.externs
            .iter()
            .map(|(_, artifact, _)| Path::new(&**artifact))
    }

    /// The basis of `request`'s resolution, which builds the project's
    /// dependencies with the pinned nightly as the metadata probe does.
    pub fn capture(
        request: crate::cargo::Request,
        lane: &mut crate::cargo::Lane,
        native: &crate::process::Native,
        limits: &crate::limits::Limits,
    ) -> crate::Result<Self> {
        let resolution = request.resolve(lane, native, limits)?;
        resolution
            .expansion()
            .map(Self::of)
            .ok_or_else(|| crate::Error::Request {
                reason: "the resolution built no crate a derive may come from".into(),
            })
    }

    /// The basis the last expansion in `directory` ran against, while each
    /// artifact and directory it names is still there.
    #[must_use]
    pub fn kept(directory: &Path) -> Option<Self> {
        let bytes = std::fs::read(directory.join(BASIS)).ok()?;
        let basis = serde_json::from_slice::<Self>(&bytes).ok()?;
        let artifacts = basis
            .externs
            .iter()
            .all(|(_, artifact, _)| Path::new(&**artifact).is_file());
        let directories = basis
            .dependency_directories
            .iter()
            .all(|path| path.is_dir());
        (artifacts && directories).then_some(basis)
    }

    /// Keep this basis in `directory`, where the next expansion finds it
    /// without resolving the project again.
    pub fn keep(&self, directory: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(directory)?;
        let bytes = serde_json::to_vec(self).map_err(std::io::Error::other)?;
        let staged = directory.join(format!(".{BASIS}.{}", std::process::id()));
        std::fs::write(&staged, bytes)?;
        std::fs::rename(&staged, directory.join(BASIS))
    }
}

/// rustc's expansion of each of `items`, in order, through `native`'s
/// driver, which expands them all in one run.
#[cfg(feature = "rustc-driver")]
pub fn expand(
    items: &[Item],
    basis: &Basis,
    edition: &str,
    directory: &Path,
    native: &crate::process::Native,
    limits: &crate::limits::Limits,
) -> crate::Result<Vec<request::Expanded>> {
    let items = items.iter().collect::<Vec<_>>();
    run(&items, basis, edition, directory, native, limits)
}

/// rustc's expansion of each of `items`, in order, from one driver run over
/// the small crate of them all.
#[cfg(feature = "rustc-driver")]
fn run(
    items: &[&Item],
    basis: &Basis,
    edition: &str,
    directory: &Path,
    native: &crate::process::Native,
    limits: &crate::limits::Limits,
) -> crate::Result<Vec<request::Expanded>> {
    let (text, spans) = crate_source(items);
    let roots = items
        .iter()
        .flat_map(|item| &item.derives)
        .map(|(_, path)| path.split("::").next().unwrap_or_default())
        .collect::<std::collections::BTreeSet<_>>();
    let expanded = drive(&text, &roots, basis, edition, directory, native, limits)?;
    Ok(split(expanded, items, &spans))
}

/// rustc's expansion of the small crate `text`, whose macros come from the
/// crates `roots` names, in one driver run in a directory of its own under
/// `directory`.
#[cfg(feature = "rustc-driver")]
fn drive(
    text: &str,
    roots: &std::collections::BTreeSet<&str>,
    basis: &Basis,
    edition: &str,
    directory: &Path,
    native: &crate::process::Native,
    limits: &crate::limits::Limits,
) -> crate::Result<request::Expanded> {
    use crate::process::{Command, Program, ToolPaths as _};
    let protocol = crate::extract::protocol::SCHEMA;
    let io = |operation: &'static str, path: &Path| {
        let path = path.to_path_buf();
        move |source| crate::Error::Io {
            operation,
            path,
            source,
        }
    };
    let work = directory.join(format!(".expand.{}", std::process::id()));
    std::fs::create_dir_all(&work).map_err(io("create the expansion directory", &work))?;
    let source_path = work.join("lib.rs");
    std::fs::write(&source_path, text).map_err(io("write the expansion source", &source_path))?;
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
        // A crate the program reaches only through another's hidden
        // segment, which rustc finds among the dependencies (#2290).
        crates: roots
            .iter()
            .filter(|root| !basis.names(root))
            .map(|root| (*root).to_owned())
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
    let expanded = ran
        .and_then(|output| output.checked(Program::Driver))
        .and_then(|_| {
            let bytes = std::fs::read(&out).map_err(io("read the expansion", &out))?;
            serde_json::from_slice::<request::Expanded>(&bytes).map_err(|error| {
                crate::Error::Request {
                    reason: format!("the driver's expansion: {error}").into(),
                }
            })
        });
    let _ = std::fs::remove_dir_all(&work);
    expanded
}

#[cfg_attr(
    not(feature = "rustc-driver"),
    allow(dead_code, reason = "only the driver's build runs an expansion")
)]
/// The driver's expansion of the small crate of `items` as each item's own:
/// the derives reported in the bytes its module `spans`. std's own derives
/// (`Debug`, `Clone`) expand in the small crate too; only the crates'
/// derives an item names are its expansion here.
fn split(
    expanded: request::Expanded,
    items: &[&Item],
    spans: &[std::ops::Range<u32>],
) -> Vec<request::Expanded> {
    let mut each = items
        .iter()
        .map(|_| request::Expanded {
            protocol: expanded.protocol,
            expansions: Vec::new(),
            calls: Vec::new(),
        })
        .collect::<Vec<_>>();
    for expansion in expanded.expansions {
        let Some(index) = spans.iter().position(|span| span.contains(&expansion.at)) else {
            continue;
        };
        let named = items[index]
            .derives
            .iter()
            .any(|(written, _)| macro_name(written) == macro_name(&expansion.derive));
        if named {
            each[index].expansions.push(expansion);
        }
    }
    each
}

#[cfg(test)]
mod tests {
    use super::{
        built, crate_source, helpers, kept, request, source, source_path, source_text, split, Item,
        Kept,
    };

    #[test]
    fn an_expansion_source_names_its_macro_and_its_item_and_follows_it() {
        let item = "struct A;";
        let expansion = request::Expansion {
            derive: "Serialize".to_owned(),
            at: 0,
            items: vec!["impl A for B {}".to_owned()],
            helpers: vec!["serde".to_owned(), "skip".to_owned()],
        };
        let text = source_text("serde::Serialize", item, "a1", Some(&expansion));
        assert!(text.starts_with("// derive serde::Serialize "), "{text}");
        assert_eq!(built(&text), Some(("serde::Serialize", "a1")));
        // It names the attributes the derive declares (#2516).
        assert_eq!(helpers(&text).collect::<Vec<_>>(), ["serde", "skip"]);
        let none = source_text("serde::Serialize", item, "a1", None);
        assert_eq!(helpers(&none).count(), 0);
        assert_eq!(built(&none), Some(("serde::Serialize", "a1")));
        assert!(text.ends_with("\nimpl A for B {}\n"), "{text}");
        assert_eq!(kept(&text, item), Some(Kept::Current("serde::Serialize")));
        // An edit to the item leaves the source stale, not another's.
        assert_eq!(
            kept(&text, "struct A { b: u8 }"),
            Some(Kept::Stale("serde::Serialize"))
        );
        assert_eq!(kept("impl A for B {}\n", item), None);
        // The source's path is its item's and derive's, whatever the
        // item's text: an edit to the item writes the same source.
        let project = std::path::Path::new("/p");
        let path = source_path(project, "app::A", "Serialize");
        assert!(path.starts_with("/p/target/rawr/expand"));
        assert_eq!(path, source_path(project, "app::A", "Serialize"));
        assert_ne!(path, source_path(project, "app::A", "Deserialize"));
        assert_ne!(path, source_path(project, "app::B", "Serialize"));
        assert_ne!(path, source_path(project, "A", "Serialize"));
    }

    fn task() -> Item {
        Item {
            path: "Task".to_owned(),
            text: "#[derive(Debug, Serialize)]\nstruct Task { done: bool }".to_owned(),
            derives: vec![("Serialize".to_owned(), "serde::Serialize".to_owned())],
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
            path: "Task".to_owned(),
            text: "#[derive(serde::Serialize)]\nstruct Task;".to_owned(),
            derives: vec![("serde::Serialize".to_owned(), "serde::Serialize".to_owned())],
        };
        assert_eq!(
            source(&pathed),
            "#[derive(serde::Serialize)]\nstruct Task;\n"
        );
    }

    #[test]
    fn every_item_expands_in_one_crate_and_each_takes_its_own_derives() {
        let loaded = Item {
            path: "app::Shape".to_owned(),
            text: "#[derive(Load)]\nenum Shape { Dot }".to_owned(),
            derives: vec![("Load".to_owned(), "serde::Deserialize".to_owned())],
        };
        let items = [&task(), &loaded];
        let (text, spans) = crate_source(&items);
        // Each item's `use`s are its module's own.
        assert!(text.starts_with(
            "mod __rawr_expand_0 {\n#[allow(unused_imports)]\nuse serde::Serialize as Serialize;\n"
        ));
        assert!(text.contains(
            "mod __rawr_expand_1 {\n#[allow(unused_imports)]\nuse serde::Deserialize as Load;\n"
        ));
        let at = |needle: &str| u32::try_from(text.find(needle).unwrap()).unwrap();
        let expansion = |derive: &str, at: u32| request::Expansion {
            derive: derive.to_owned(),
            at,
            items: vec![format!("impl {derive} {at}")],
            helpers: Vec::new(),
        };
        let expanded = request::Expanded {
            protocol: 53,
            expansions: vec![
                expansion("Debug", at("Debug")),
                expansion("Serialize", at("Serialize)]")),
                // rustc names an expansion by the name the item writes.
                expansion("Load", at("Load)]")),
            ],
            calls: Vec::new(),
        };
        let each = split(expanded, &items, &spans);
        let derives = |index: usize| {
            each[index]
                .expansions
                .iter()
                .map(|expansion| expansion.derive.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(derives(0), ["Serialize"]);
        assert_eq!(derives(1), ["Load"]);
        assert!(each.iter().all(|one| one.protocol == 53));
    }
}
