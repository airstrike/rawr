//! The runner a program that uses no crates runs on in a browser: rawr and
//! std alone, the language `rawr run` runs such a program on. It covers
//! every such program, since none of them reaches a crate, so one is built
//! for each rawr version, in rawr's home, and every program with no
//! dependencies shares it. A page then costs such a program no build of
//! its own once that runner is built.
//!
//! The runner is built from a package rawr writes, as a project's is: a
//! marked `main` and an empty library, which is the one library the
//! import pipeline builds and extracts a runner from. The library has no
//! items, so the runner covers nothing past std.

use std::path::{Path, PathBuf};

use super::Error;

/// The package's name, which names its library crate too.
const NAME: &str = "rawr-plain-runner";

/// The package's files, by their paths in it.
const FILES: [(&str, &str); 3] = [
    (
        "Cargo.toml",
        "# The package rawr builds the browser's runner of rawr and std from.\n\
         [package]\nname = \"rawr-plain-runner\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\
         publish = false\n\n[workspace]\n",
    ),
    ("src/lib.rs", ""),
    ("src/main.rs", "// rawr\nfn main() {}\n"),
];

/// Write the package under `home`, rawr's home, for this rawr version,
/// and give its directory. A file that holds its text already is left as
/// it is, so a built runner stays built. Another version's package goes,
/// with its build: no rawr of that version runs a page on it again.
pub(super) fn package(home: &Path) -> Result<PathBuf, Error> {
    let web = home.join("web");
    let name = format!("{NAME}-{}", env!("CARGO_PKG_VERSION"));
    let others = std::fs::read_dir(&web).into_iter().flatten().flatten();
    for other in others {
        let file = other.file_name();
        let file = file.to_string_lossy();
        if file != name && file.starts_with(&format!("{NAME}-")) {
            let _ = std::fs::remove_dir_all(other.path());
        }
    }
    let directory = web.join(name);
    for (name, text) in FILES {
        let path = directory.join(name);
        if std::fs::read_to_string(&path).is_ok_and(|held| held == text) {
            continue;
        }
        let written = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&path, text));
        written.map_err(|source| Error::File { path, source })?;
    }
    Ok(directory)
}

#[cfg(test)]
mod tests {
    /// The package is a marked binary beside an empty library, written
    /// once: a second write leaves its files as they were, and another
    /// version's package is removed.
    #[test]
    fn the_plain_runners_package_is_written_once() {
        let home = std::env::temp_dir().join(format!("rawr-web-base-{}", std::process::id()));
        let old = home.join("web").join(format!("{}-0.0.1", super::NAME));
        std::fs::create_dir_all(old.join("target")).unwrap();
        let directory = super::package(&home).expect("the package is written");
        assert!(!old.exists(), "another version's package goes");
        assert!(directory.starts_with(home.join("web")));
        let main = directory.join("src").join("main.rs");
        assert_eq!(
            std::fs::read_to_string(&main).unwrap(),
            "// rawr\nfn main() {}\n"
        );
        assert_eq!(
            std::fs::read_to_string(directory.join("src").join("lib.rs")).unwrap(),
            ""
        );
        let written = std::fs::metadata(&main).unwrap().modified().unwrap();
        assert_eq!(super::package(&home).unwrap(), directory);
        assert_eq!(
            std::fs::metadata(&main).unwrap().modified().unwrap(),
            written
        );
        std::fs::remove_dir_all(home).unwrap();
    }
}
