//! Why a program couldn't be built for a browser, packed or served.

use std::path::PathBuf;
use std::process::ExitCode;

use rawr_cli::color;

/// What stopped `rawr run --web` from building, packing or serving.
#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    /// A program the command runs couldn't start.
    #[error("cannot run `{program}`: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },
    /// The runner's build failed; its own output said why.
    #[error("the program's runner for the browser didn't build")]
    Build,
    /// The build ended well but left no runner where it was asked to.
    #[error("the runner's build wrote no runner to {}", .0.display())]
    Missing(PathBuf),
    /// The runner names no wasm-bindgen it links.
    #[error("the runner {} names no `wasm-bindgen` it links", .0.display())]
    Unlinked(PathBuf),
    /// wasm-bindgen's CLI at the version the runner links didn't install.
    #[error("wasm-bindgen-cli {0} didn't install")]
    Install(String),
    /// wasm-bindgen didn't pack the runner.
    #[error("wasm-bindgen didn't pack the runner {}", .0.display())]
    Pack(PathBuf),
    /// A file or directory couldn't be read or written.
    #[error("{}: {source}", path.display())]
    File {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// Cargo couldn't say how it builds the package for the browser.
    #[error("{0}")]
    Cargo(String),
    /// The package's files couldn't be read.
    #[error("{0}")]
    Read(#[from] rawr::universe::Error),
    /// No port to serve the page on.
    #[error("cannot serve the page on {address}: {source}")]
    Serve {
        address: String,
        #[source]
        source: std::io::Error,
    },
}

/// Why `rawr run --web` ended without serving: the program's report, the
/// command's own error, or a program it can't run in a browser.
pub(crate) enum Failure {
    Report(Box<rawr::diagnostic::Report>),
    Web(Error),
    Usage(&'static str),
}

impl Failure {
    /// Say why, and give the status the command ends with.
    pub(crate) fn exit(self) -> ExitCode {
        match self {
            Self::Report(report) => rawr_cli::diagnostic::emit(&report),
            Self::Web(error) => eprintln!("{}: {error}", color::label(color::Level::Error)),
            Self::Usage(message) => {
                eprintln!("{}: {message}", color::label(color::Level::Error));
            }
        }
        ExitCode::FAILURE
    }
}

impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        Self::Web(error)
    }
}

impl From<rawr::diagnostic::Report> for Failure {
    fn from(report: rawr::diagnostic::Report) -> Self {
        Self::Report(Box::new(report))
    }
}
