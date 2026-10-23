//! Programs as `rawr` loads them. rawr-iced loads through the same parse
//! cells: a program `rawr` handed over is read from the cells `rawr` wrote,
//! and what rawr-iced had to parse is written back for the next command.

use std::path::PathBuf;

pub(crate) use rawr_cli::base::{check, run, Failed, Loaded, Session};

/// Load the program at `path` once, through its parse cells.
pub(crate) fn load(path: Option<&PathBuf>) -> Result<Loaded, rawr::diagnostic::Report> {
    let loaded = rawr_cli::base::load(path)?;
    rawr_cli::cells::keep("rawr-iced", &loaded.cells, &loaded.graph);
    Ok(loaded)
}

/// What runs a program: the base alone, or rawr-iced on top.
pub(crate) enum Program {
    Base(Loaded),
    Layered(Loaded),
}

/// Load the program at `path` and decide whether rawr-iced serves it.
/// rawr-iced runs a program against the iced its project's `cargo build`
/// builds, so a program it serves must have a Cargo.toml.
pub(crate) fn layer(path: Option<&PathBuf>) -> Result<Program, rawr::diagnostic::Report> {
    let loaded = load(path)?;
    if !crate::layer::claims().claim(loaded.sheet.facts()) {
        return Ok(Program::Base(loaded));
    }
    if !loaded.graph.declares_rust_import() {
        return Err(without_cargo(loaded.target.source().path()));
    }
    Ok(Program::Layered(loaded))
}

/// A program that uses iced but has no Cargo.toml naming it.
pub(crate) fn without_cargo(path: &std::path::Path) -> rawr::diagnostic::Report {
    rawr::diagnostic::Report::error(
        rawr::diagnostic::code::ENVIRONMENT_PROJECT_NOT_FOUND,
        format!(
            "`{}` uses iced, and iced comes from a Cargo project: Rawr runs the program \
             against the iced its `cargo build` builds. Start one with `rawr new <name>`, \
             or add a Cargo.toml that depends on iced beside the program.",
            path.display()
        ),
    )
    .finish()
}

/// A live session of rawr-iced over a program it serves.
pub(crate) fn session(sheet: rawr::layer::Sheet) -> Session {
    Session::new(crate::layer::claims(), sheet)
}
