//! Where a program's parse cells live, and what a load through them did.
//!
//! The cells of a program are in `<target>/rawr/parse`, `<target>` being the
//! Cargo target directory of the program's package, as for every other
//! product Rawr keeps for a project. `cargo clean` removes them.

use std::path::{Path, PathBuf};

use rawr::package::{Cells, Graph};
use rawr::project;

/// The parse cells directory of the program `target` selects.
#[must_use]
pub fn directory(target: &project::Target) -> PathBuf {
    let root = Graph::root_of(target);
    let root = std::fs::canonicalize(&root).unwrap_or(root);
    let variables: Vec<(&str, String)> = ["CARGO_TARGET_DIR", "CARGO_BUILD_TARGET_DIR"]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok().map(|value| (key, value)))
        .collect();
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".cargo")));
    rawr_location::target_directory(
        &root,
        |key| {
            variables
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| value.as_str())
        },
        cargo_home.as_deref(),
    )
    .join("rawr")
    .join("parse")
}

/// Whether `RAWR_VERBOSE` asks the command to say what it did.
#[must_use]
pub fn verbose() -> bool {
    std::env::var_os("RAWR_VERBOSE").is_some_and(|value| !value.is_empty() && value != "0")
}

/// Write the cells of the sources `graph` parsed, so the next process to
/// load the program reads them. With `RAWR_VERBOSE`, say what loading did
/// and what was written, as `program`.
pub fn keep(program: &str, cells: &Cells, graph: &Graph) {
    tell(program, cells);
    if cells.counts().parsed == 0 {
        return;
    }
    match cells.save(graph) {
        Ok(written) if verbose() => eprintln!(
            "{program}: wrote {written} parse cells to {}",
            cells.directory().display()
        ),
        Err(error) if verbose() => eprintln!(
            "{program}: cannot write parse cells to {}: {error}",
            cells.directory().display()
        ),
        _ => {}
    }
}

/// With `RAWR_VERBOSE`, say how a load through `cells` went, as `program`.
pub fn tell(program: &str, cells: &Cells) {
    if !verbose() {
        return;
    }
    let counts = cells.counts();
    eprintln!(
        "{program}: parsed {} sources, read {} from parse cells",
        counts.parsed, counts.kept
    );
    if counts.stale > 0 {
        eprintln!(
            "{program}: {} parse cells came from another build of rawr; parsed them again",
            counts.stale
        );
    }
}
