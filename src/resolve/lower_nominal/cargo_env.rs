//! What Cargo tells rustc about the crate a file is in: the `CARGO_PKG_*`,
//! `CARGO_MANIFEST_DIR` and `CARGO_CRATE_NAME` variables `env!` reads,
//! and the path `file!()` gives.

use std::path::{Path, PathBuf};

use toml_edit::{Document, Item};

/// The package whose `Cargo.toml` is nearest above `file`, and its parsed
/// manifest.
struct Package {
    directory: PathBuf,
    document: Document<String>,
}

impl Package {
    fn of(file: &Path) -> Option<Self> {
        file.ancestors().skip(1).find_map(|directory| {
            let text = std::fs::read_to_string(directory.join("Cargo.toml")).ok()?;
            let document = Document::parse(text).ok()?;
            document.as_table().get("package")?.as_table_like()?;
            Some(Self {
                directory: directory.to_path_buf(),
                document,
            })
        })
    }

    fn package(&self) -> Option<&dyn toml_edit::TableLike> {
        self.document.as_table().get("package")?.as_table_like()
    }

    /// `[package] key`, or the workspace's `[workspace.package] key` when
    /// the package inherits it (`key.workspace = true`).
    fn field(&self, key: &str) -> Option<Item> {
        let item = self.package()?.get(key)?;
        let inherited = item
            .as_table_like()
            .and_then(|table| table.get("workspace"))
            .and_then(Item::as_bool)
            == Some(true);
        if !inherited {
            return Some(item.clone());
        }
        let root = self.workspace_root()?;
        let text = std::fs::read_to_string(root.join("Cargo.toml")).ok()?;
        let document = Document::parse(text).ok()?;
        document
            .as_table()
            .get("workspace")?
            .as_table_like()?
            .get("package")?
            .as_table_like()?
            .get(key)
            .cloned()
    }

    fn text(&self, key: &str) -> Option<String> {
        self.field(key)?.as_str().map(str::to_owned)
    }

    fn name(&self) -> String {
        self.text("name").unwrap_or_default()
    }

    /// The directory of the workspace this package is a member of: the
    /// nearest manifest with a `[workspace]` at or above the package.
    fn workspace_root(&self) -> Option<PathBuf> {
        self.directory.ancestors().find_map(|directory| {
            let text = std::fs::read_to_string(directory.join("Cargo.toml")).ok()?;
            let document = Document::parse(text).ok()?;
            document
                .as_table()
                .get("workspace")
                .is_some()
                .then(|| directory.to_path_buf())
        })
    }

    /// The target whose root is `entry`: its crate name, and its name when
    /// it is a binary.
    fn target(&self, entry: &Path) -> (String, Option<String>) {
        let package = self.name();
        let relative = entry.strip_prefix(&self.directory).unwrap_or(entry);
        // A `[[bin]]` naming its path.
        if let Some(bins) = self
            .document
            .as_table()
            .get("bin")
            .and_then(Item::as_array_of_tables)
        {
            for bin in bins.iter() {
                let path = bin.get("path").and_then(Item::as_str).map(Path::new);
                if path == Some(relative)
                    && let Some(name) = bin.get("name").and_then(Item::as_str)
                {
                    return (name.replace('-', "_"), Some(name.to_owned()));
                }
            }
        }
        let parts: Vec<&str> = relative
            .components()
            .filter_map(|part| part.as_os_str().to_str())
            .collect();
        let stem = || {
            relative
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_default()
                .to_owned()
        };
        match parts[..] {
            ["src", "main.rs"] => (package.replace('-', "_"), Some(package)),
            ["src", "bin", _] => {
                let name = stem();
                (name.replace('-', "_"), Some(name))
            }
            ["src", "bin", name, "main.rs"] => (name.replace('-', "_"), Some(name.to_owned())),
            ["src", "lib.rs"] => {
                let library = self
                    .document
                    .as_table()
                    .get("lib")
                    .and_then(|lib| lib.get("name"))
                    .and_then(Item::as_str)
                    .map_or_else(|| package.clone(), str::to_owned);
                (library.replace('-', "_"), None)
            }
            _ => (stem().replace('-', "_"), None),
        }
    }

    /// The variable `name` as Cargo sets it for the target rooted at
    /// `entry`.
    fn variable(&self, name: &str, entry: &Path) -> Option<String> {
        let version = self.text("version").unwrap_or_else(|| "0.0.0".into());
        // `1.2.3-beta.1+build`: the build metadata is no part of these.
        let release = version.split('+').next().unwrap_or_default();
        let (core, pre) = release.split_once('-').unwrap_or((release, ""));
        let mut numbers = core.split('.');
        let (major, minor, patch) = (numbers.next(), numbers.next(), numbers.next());
        let text = |key: &str| Some(self.text(key).unwrap_or_default());
        match name {
            "CARGO_MANIFEST_DIR" => Some(self.directory.to_string_lossy().into_owned()),
            "CARGO_MANIFEST_PATH" => Some(
                self.directory
                    .join("Cargo.toml")
                    .to_string_lossy()
                    .into_owned(),
            ),
            "CARGO_PKG_NAME" => Some(self.name()),
            "CARGO_PKG_VERSION" => Some(version.clone()),
            "CARGO_PKG_VERSION_MAJOR" => major.map(str::to_owned),
            "CARGO_PKG_VERSION_MINOR" => minor.map(str::to_owned),
            "CARGO_PKG_VERSION_PATCH" => patch.map(str::to_owned),
            "CARGO_PKG_VERSION_PRE" => Some(pre.to_owned()),
            // Cargo joins the authors with `:`.
            "CARGO_PKG_AUTHORS" => Some(
                self.field("authors")
                    .and_then(|authors| {
                        authors.as_array().map(|authors| {
                            authors
                                .iter()
                                .filter_map(|author| author.as_str())
                                .collect::<Vec<_>>()
                                .join(":")
                        })
                    })
                    .unwrap_or_default(),
            ),
            "CARGO_PKG_DESCRIPTION" => text("description"),
            "CARGO_PKG_HOMEPAGE" => text("homepage"),
            "CARGO_PKG_REPOSITORY" => text("repository"),
            "CARGO_PKG_LICENSE" => text("license"),
            "CARGO_PKG_LICENSE_FILE" => text("license-file"),
            "CARGO_PKG_RUST_VERSION" => text("rust-version"),
            // `readme = false` has none; without the key, Cargo finds one.
            "CARGO_PKG_README" => Some(match self.field("readme") {
                Some(readme) => readme.as_str().unwrap_or_default().to_owned(),
                None => ["README.md", "README.txt", "README"]
                    .into_iter()
                    .find(|readme| self.directory.join(readme).is_file())
                    .unwrap_or_default()
                    .to_owned(),
            }),
            "CARGO_CRATE_NAME" => Some(self.target(entry).0),
            "CARGO_BIN_NAME" => self.target(entry).1,
            "CARGO_PRIMARY_PACKAGE" => Some("1".into()),
            _ => None,
        }
    }
}

/// The variable `name` that Cargo sets when it builds the target rooted at
/// `entry`, for code in `file`: `None` outside a package, or for a variable
/// Cargo does not set for that target.
pub(super) fn variable(file: &Path, entry: &Path, name: &str) -> Option<Option<String>> {
    let package = Package::of(file)?;
    Some(package.variable(name, entry))
}

/// `file!()` of code in `file`: its path from the workspace root (or the
/// package's directory), as Cargo passes it to rustc; outside a package,
/// its path from the current directory, as `rustc x.rs` would be given.
pub(super) fn file_path(file: &Path) -> String {
    let base = match Package::of(file) {
        Some(package) => package
            .workspace_root()
            .unwrap_or_else(|| package.directory.clone()),
        None => std::env::current_dir().unwrap_or_default(),
    };
    file.strip_prefix(&base)
        .unwrap_or(file)
        .to_string_lossy()
        .into_owned()
}

/// The crate name of the target rooted at `entry`, which `module_path!()`
/// starts with: rustc's name of a lone file is its stem.
pub(super) fn crate_name(entry: &Path) -> String {
    match Package::of(entry) {
        Some(package) => package.target(entry).0,
        None => entry
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .replace('-', "_"),
    }
}
