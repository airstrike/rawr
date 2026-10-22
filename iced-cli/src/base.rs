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
pub(crate) fn layer(path: Option<&PathBuf>) -> Result<Program, rawr::diagnostic::Report> {
    let loaded = load(path)?;
    Ok(if crate::layer::claims().claim(loaded.sheet.facts()) {
        Program::Layered(loaded)
    } else {
        Program::Base(loaded)
    })
}

/// A live session of rawr-iced over a program it serves.
pub(crate) fn session(sheet: rawr::layer::Sheet) -> Session {
    Session::new(crate::layer::claims(), sheet)
}
