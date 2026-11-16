//! The program's package as the page runs it: its crate root and the files
//! `rawr::universe` reads for it, which the page hands the runner's
//! universe as text, since a page reaches no files of this machine.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

/// The package's files as read, its crate root among them.
#[derive(Debug, Eq, PartialEq)]
pub(super) struct Package {
    entry: PathBuf,
    files: Vec<(PathBuf, Box<str>)>,
}

impl Package {
    /// Read the package of the crate root at `entry`.
    pub(super) fn read(entry: &Path) -> Result<Self, rawr::universe::Error> {
        Ok(Self {
            entry: entry.to_path_buf(),
            files: rawr::universe::package_files(entry)?,
        })
    }

    /// The program as the page starts it: `{"entry":..,"files":[..]}`.
    #[must_use]
    pub(super) fn program(&self) -> Value {
        json!({ "entry": self.entry.to_string_lossy(), "files": self.files() })
    }

    /// The files as the page's universe takes them, `[path, text]` pairs.
    #[must_use]
    pub(super) fn files(&self) -> Value {
        Value::Array(
            self.files
                .iter()
                .map(|(path, text)| json!([path.to_string_lossy(), text]))
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Package;

    /// The page gets the manifest and every Rust file of the crate's
    /// source directory, as text, by the paths it compiles them at.
    #[test]
    fn a_package_is_its_manifest_and_its_rust_files() {
        let root = std::env::temp_dir().join(format!("rawr-web-package-{}", std::process::id()));
        let source = root.join("src");
        std::fs::create_dir_all(source.join("view")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"app\"\n").unwrap();
        std::fs::write(source.join("main.rs"), "// rawr\nmod view;\nfn main() {}\n").unwrap();
        std::fs::write(source.join("view.rs"), "mod list;\n").unwrap();
        std::fs::write(source.join("view").join("list.rs"), "").unwrap();
        std::fs::write(source.join("notes.txt"), "not Rust").unwrap();
        let package = Package::read(&source.join("main.rs")).expect("the package reads");
        let program = package.program();
        let paths: Vec<_> = program["files"]
            .as_array()
            .expect("the files are an array")
            .iter()
            .map(|file| file[0].as_str().expect("a path").to_owned())
            .collect();
        let path = |path: std::path::PathBuf| path.to_string_lossy().into_owned();
        assert_eq!(
            paths,
            [
                path(root.join("Cargo.toml")),
                path(source.join("main.rs")),
                path(source.join("view").join("list.rs")),
                path(source.join("view.rs")),
            ]
        );
        assert_eq!(program["entry"], path(source.join("main.rs")));
        assert_eq!(program["files"][1][1], "// rawr\nmod view;\nfn main() {}\n");
        std::fs::remove_dir_all(root).unwrap();
    }
}
