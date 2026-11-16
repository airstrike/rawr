//! The universe runner: one runner built ahead of time that serves every
//! program whose native calls it covers, as `?play` runs snippets (#434,
//! #436).
//!
//! It names each script type by its index in the program's definition
//! order (`__RawrScript0`, `__RawrScript1`, ..), so the runner names no
//! script type and two programs that differ only in their type names run
//! on one binary. A module with its own `main` is a program of its own
//! there, and counts its types from zero, so one runner serves each of
//! the programs it holds as it serves a snippet that is one of them
//! (#1000). A project's runner names script types as the program does,
//! so `type_name` and iced's window title are the program's (D594).
//!
//! The setting is one per process. `rawr-import` takes it from
//! `RAWR_UNIVERSE_RUNNER=1` when it builds the runner; the generated runner
//! sets it itself before it compiles a program, so both compile the
//! program alike.
//!
//! A caller that takes the runner elsewhere, as ?play's image and the
//! browser's universe do, names a directory in `RAWR_UNIVERSE_OUT`. Each
//! runner `rawr-import` builds or reuses is then written there as `runner`
//! (#1752), beside the `Cargo.lock` it was built from, which names the
//! `wasm-bindgen` a browser's runner links. A project's runner is written
//! there too, as `rawr run --web` takes it. Where rawr keeps the runner
//! itself is rawr's own business.

#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::OnceLock;

static UNIVERSE: AtomicBool = AtomicBool::new(false);

#[cfg(not(target_arch = "wasm32"))]
static DESTINATION: OnceLock<PathBuf> = OnceLock::new();

/// The variable that makes `rawr-import` build a universe runner.
pub const VARIABLE: &str = "RAWR_UNIVERSE_RUNNER";

/// The variable that names the directory a universe runner is written to.
pub const DIRECTORY: &str = "RAWR_UNIVERSE_OUT";

/// The runner's file name in that directory.
pub const RUNNER: &str = if cfg!(windows) {
    "runner.exe"
} else {
    "runner"
};

/// Make this process compile programs as a universe runner does.
pub fn set() {
    UNIVERSE.store(true, Ordering::Relaxed);
}

/// Whether this process compiles programs as a universe runner does.
#[must_use]
pub fn enabled() -> bool {
    UNIVERSE.load(Ordering::Relaxed)
}

/// Write each universe runner this process builds to `directory`, made
/// absolute here. The first call wins.
#[cfg(not(target_arch = "wasm32"))]
pub fn deliver_into(directory: &Path) {
    let directory = std::path::absolute(directory).unwrap_or_else(|_| directory.to_path_buf());
    let _ = DESTINATION.set(directory);
}

/// Whether a caller takes this process's runners from a directory
/// [`deliver_into`] named.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn delivers() -> bool {
    DESTINATION.get().is_some()
}

/// Write `built`'s runner to the directory [`deliver_into`] named, as
/// [`RUNNER`], beside the `Cargo.lock` it was built from as [`LOCK`], and
/// give the runner's path. A process that named none writes nothing. Each
/// file is replaced whole, so a reader never sees part of one.
#[cfg(not(target_arch = "wasm32"))]
pub fn deliver<B>(
    built: &crate::runner::Manifest<crate::runner::Built, B>,
) -> Result<Option<PathBuf>, Error> {
    let Some(directory) = DESTINATION.get() else {
        return Ok(None);
    };
    if let Some(lock) = built.files.iter().find(|file| file.path() == LOCK) {
        replace(directory, LOCK, |partial| {
            std::fs::write(partial, lock.bytes())
        })?;
    }
    let executable = built.artifact().executable();
    replace(directory, RUNNER, |partial| {
        std::fs::copy(executable, partial).map(drop)
    })
    .map(Some)
}

/// The name of the lock beside a runner [`deliver`] writes.
pub const LOCK: &str = "Cargo.lock";

/// Write `directory`'s file `name` whole with `write`, which writes a
/// partial file it is renamed from, and give its path.
#[cfg(not(target_arch = "wasm32"))]
fn replace(
    directory: &Path,
    name: &str,
    write: impl FnOnce(&Path) -> std::io::Result<()>,
) -> Result<PathBuf, Error> {
    let path = directory.join(name);
    let partial = directory.join(format!(".{name}.{}", std::process::id()));
    let written = std::fs::create_dir_all(directory)
        .and_then(|()| write(&partial))
        .and_then(|()| std::fs::rename(&partial, &path));
    match written {
        Ok(()) => Ok(path),
        Err(source) => {
            let _ = std::fs::remove_file(&partial);
            Err(Error { path, source })
        }
    }
}

/// A universe runner that could not be written where its caller asked.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, thiserror::Error)]
#[error("cannot write the universe runner to {}", path.display())]
pub struct Error {
    path: PathBuf,
    #[source]
    source: std::io::Error,
}
