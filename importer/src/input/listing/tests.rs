use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::files;

/// A package directory under the target directory, removed when dropped.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(name: &str, contents: &[(&str, &str)]) -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/test-scratch")
            .join(format!("listing-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for (path, text) in contents {
            let path = root.join(path);
            fs::create_dir_all(path.parent().expect("a listed path has a parent"))
                .expect("the fixture directory creates");
            fs::write(path, text).expect("the fixture file writes");
        }
        let root = fs::canonicalize(root).expect("the fixture root canonicalizes");
        Self { root }
    }

    fn git(&self, arguments: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(arguments)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .status()
            .expect("git runs");
        assert!(status.success(), "git {arguments:?}");
    }

    fn listed(&self) -> Vec<String> {
        files(&self.root)
            .expect("the package lists")
            .iter()
            .map(|path| path.to_str().expect("fixture paths are text").to_owned())
            .collect()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

const MANIFEST: &str = "[package]\nname = \"listed\"\nversion = \"0.1.0\"\n";

#[test]
fn a_git_package_lists_what_git_tracks_or_would_track_and_no_nested_package() {
    let scratch = Scratch::new(
        "git",
        &[
            ("Cargo.toml", MANIFEST),
            ("src/lib.rs", ""),
            (".gitignore", "*.log\n"),
            (".hidden", ""),
            ("build.log", ""),
            ("notes.txt", ""),
            ("target/tracked", ""),
            ("target/debug/built", ""),
            ("nested/Cargo.toml", "[package]\nname = \"nested\"\n"),
            ("nested/src/lib.rs", ""),
            ("Cargo.lock", ""),
        ],
    );
    scratch.git(&["init", "-q"]);
    scratch.git(&["add", "Cargo.toml", "src/lib.rs", ".gitignore", ".hidden"]);
    scratch.git(&["add", "-f", "target/tracked"]);
    assert_eq!(
        scratch.listed(),
        [
            ".gitignore",
            ".hidden",
            "Cargo.toml",
            "notes.txt",
            "src/lib.rs",
            "target/tracked"
        ],
        "ignored, untracked under target, nested and an untracked top-level lock are left out"
    );
}

#[test]
fn a_package_outside_git_is_walked_without_hidden_names_target_or_nested_packages() {
    let scratch = Scratch::new(
        "walk",
        &[
            ("Cargo.toml", MANIFEST),
            ("src/lib.rs", ""),
            (".hidden", ""),
            ("assets/.cache/blob", ""),
            ("assets/target/kept", ""),
            ("target/debug/built", ""),
            ("nested/Cargo.toml", "[package]\nname = \"nested\"\n"),
            ("nested/src/lib.rs", ""),
        ],
    );
    assert_eq!(
        scratch.listed(),
        ["Cargo.toml", "assets/target/kept", "src/lib.rs"]
    );
}

#[test]
fn include_and_exclude_select_from_the_listing() {
    let excluded = Scratch::new(
        "exclude",
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"listed\"\nexclude = [\"/benches\", \"*.bin\", \"!keep.bin\"]\n",
            ),
            ("src/lib.rs", ""),
            ("benches/a.rs", ""),
            ("data/big.bin", ""),
            ("data/keep.bin", ""),
        ],
    );
    assert_eq!(
        excluded.listed(),
        ["Cargo.toml", "data/keep.bin", "src/lib.rs"]
    );
    let included = Scratch::new(
        "include",
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"listed\"\ninclude = [\"src/**/*.rs\", \".config\"]\n",
            ),
            ("src/lib.rs", ""),
            ("src/a/b.rs", ""),
            ("src/notes.md", ""),
            (".config", ""),
            ("README.md", ""),
        ],
    );
    assert_eq!(
        included.listed(),
        [".config", "Cargo.toml", "src/a/b.rs", "src/lib.rs"],
        "an include keeps hidden names it names"
    );
}

#[test]
fn an_inherited_exclude_is_read_from_the_workspace() {
    let scratch = Scratch::new(
        "inherited",
        &[
            (
                "Cargo.toml",
                "[workspace]\nmembers = [\"member\"]\n\n[workspace.package]\nexclude = [\"*.tmp\"]\n",
            ),
            (
                "member/Cargo.toml",
                "[package]\nname = \"member\"\nexclude.workspace = true\n",
            ),
            ("member/src/lib.rs", ""),
            ("member/scratch.tmp", ""),
        ],
    );
    let member = files(&scratch.root.join("member")).expect("the member lists");
    assert_eq!(
        member,
        [PathBuf::from("Cargo.toml"), PathBuf::from("src/lib.rs")]
    );
}
