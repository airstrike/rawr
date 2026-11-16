//! `rawr run --web`: a program run in a browser (#1395). The program's
//! declared dependencies, their generated bindings and its runner are built
//! for wasm32 by this executable run again with Cargo's target set, as a
//! runner for a target this machine can't run is built and not started
//! (#1753). wasm-bindgen packs the runner with rawr-web's page for a
//! project (`web/www/run.html`), and the page runs the program's marked files against
//! it through the runner's universe (`rawr::universe::browser`).
//!
//! A save sends the package's new files to the page, which reloads the
//! running program in place, its state kept, as `rawr run` reloads a save
//! (#1455). The page reports what each reload did, and the terminal says
//! it as `rawr run` does. A save that uses a crate's API the runner doesn't
//! cover builds the runner again, and the page restarts on the new one.

mod bindgen;
mod build;
mod error;
mod package;
mod server;
mod session;

use std::path::PathBuf;
use std::process::ExitCode;

pub(crate) use error::Error;

/// `rawr run --web [path]` with `options`.
pub(crate) fn run(path: Option<&PathBuf>, options: &rawr_cli::web::Options) -> ExitCode {
    match session::Session::start(path, options) {
        Ok(Some(session)) => session.serve(),
        Ok(None) => ExitCode::SUCCESS,
        Err(failure) => failure.exit(),
    }
}
