//! The program's package as the page runs it: its crate root, the files
//! `rawr::universe` reads for it, which the page hands the runner's
//! universe as text, since a page reaches no files of this machine, and
//! what its `cfg(..)`s read as Cargo builds it for the page's target: the
//! features Cargo resolves for it, a workspace member's inherited settings
//! included, and the target's options (#2121).

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

/// The package's files as read, its crate root among them.
#[derive(Debug, Eq, PartialEq)]
pub(super) struct Package {
    entry: PathBuf,
    files: Vec<(PathBuf, Box<str>)>,
    /// What its `cfg(..)`s read, as `rustc --print cfg` writes it.
    cfg: Vec<String>,
}

impl Package {
    /// Read the package of the crate root at `entry`, whose `cfg(..)`s
    /// read `features`.
    pub(super) fn read(
        entry: &Path,
        features: &rawr::project::cargo::features::Enabled,
    ) -> Result<Self, rawr::universe::Error> {
        Ok(Self {
            entry: entry.to_path_buf(),
            files: rawr::universe::package_files(entry)?,
            cfg: features.lines().collect(),
        })
    }

    /// Whether the page runs it with the same `cfg(..)`s as `other`: a
    /// package whose features changed starts again on a fresh page.
    pub(super) fn same_cfg(&self, other: &Self) -> bool {
        self.cfg == other.cfg
    }

    /// The program as the page starts it:
    /// `{"entry":..,"files":[..],"directory":..,"cfg":[..]}`, with the
    /// directory the page's errors show the files' paths from: this
    /// command's, as `rawr run` shows them.
    #[must_use]
    pub(super) fn program(&self) -> Value {
        let directory = std::env::current_dir().unwrap_or_default();
        json!({
            "entry": self.entry.to_string_lossy(),
            "files": self.files(),
            "directory": directory.to_string_lossy(),
            "cfg": self.cfg,
        })
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
    use rawr::project::cargo::features::Enabled;

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
        let features = Enabled::of_names(["loud"]);
        let package = Package::read(&source.join("main.rs"), &features).expect("the package reads");
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
        assert_eq!(program["cfg"][0], "feature=\"loud\"");
        assert_eq!(
            program["directory"],
            path(std::env::current_dir().expect("a current directory"))
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    /// A cargo script is its own package, its manifest in its frontmatter,
    /// whatever sits beside it: the page gets the script alone.
    #[test]
    fn a_script_is_its_own_package() {
        let root = std::env::temp_dir().join(format!("rawr-web-script-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"app\"\n").unwrap();
        std::fs::write(root.join("other.rs"), "fn other() {}\n").unwrap();
        let script = root.join("script.rs");
        let text = "---\n[dependencies]\nitoa = \"1\"\n---\n// rawr\nfn main() {}\n";
        std::fs::write(&script, text).unwrap();
        let program = Package::read(&script, &Enabled::default())
            .expect("the script reads")
            .program();
        let path = script.to_string_lossy().into_owned();
        assert_eq!(program["files"], serde_json::json!([[path, text]]));
        assert_eq!(program["entry"], path);
        std::fs::remove_dir_all(root).unwrap();
    }
}
