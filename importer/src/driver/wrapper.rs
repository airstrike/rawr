//! The driver as rawr's own rustc wrapper (D866): `rawr-import-driver
//! RUSTC ARGUMENTS..`, as Cargo runs a `RUSTC_WRAPPER`. A unit of one of
//! the generated crates [`record::ROOTS`] names is checked in this process,
//! and its native instances of marked generics are recorded beside its
//! artifact ([`walk`]), with what its native code uses of the marked
//! modules ([`used`]); every other unit, and every query Cargo makes of
//! rustc, runs rustc itself, so dependencies build exactly as Cargo builds
//! them and the shared cache is untouched.

#[path = "wrapper/used.rs"]
mod used;
#[path = "wrapper/walk.rs"]
mod walk;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rustc_middle::ty::TyCtxt;
use rustc_span::Symbol;

use crate::record;

/// Whether the driver was started as a wrapper: its first argument is the
/// rustc Cargo would have run, never one of the driver's own modes.
pub(crate) fn wraps(arguments: &[String]) -> bool {
    arguments
        .get(1)
        .is_some_and(|first| !first.starts_with("--"))
}

/// Run the compilation `arguments` (the driver, then rustc and its
/// arguments) asks for.
pub(crate) fn run(arguments: &[String]) -> Result<ExitCode, String> {
    let compiler = &arguments[1..];
    match recorded(compiler) {
        Some(artifact) => record(compiler, artifact),
        None => pass(compiler),
    }
}

/// The artifact of the unit `compiler` checks when it is a generated crate
/// the wrapper records: the `.rmeta` its record goes beside.
fn recorded(compiler: &[String]) -> Option<PathBuf> {
    let roots = env::var(record::ROOTS).ok()?;
    let manifest = env::var("CARGO_MANIFEST_DIR").ok()?;
    let manifest = fs::canonicalize(&manifest).unwrap_or_else(|_| PathBuf::from(manifest));
    if !roots.lines().any(|root| Path::new(root) == manifest) {
        return None;
    }
    let value = |flag: &str| {
        compiler
            .iter()
            .position(|argument| argument == flag)
            .and_then(|index| compiler.get(index + 1))
    };
    if compiler
        .iter()
        .any(|argument| argument.starts_with("--print"))
    {
        return None;
    }
    let name = value("--crate-name")?;
    let directory = value("--out-dir")?;
    let extra = compiler
        .iter()
        .find_map(|argument| argument.strip_prefix("extra-filename="))
        .unwrap_or("");
    Some(Path::new(directory).join(format!("lib{name}{extra}.rmeta")))
}

/// Run rustc itself, as Cargo would have.
fn pass(compiler: &[String]) -> Result<ExitCode, String> {
    let mut command = std::process::Command::new(&compiler[0]);
    command.args(&compiler[1..]);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        let error = command.exec();
        Err(format!("running `{}`: {error}", compiler[0]))
    }
    #[cfg(not(unix))]
    {
        let status = command
            .status()
            .map_err(|error| format!("running `{}`: {error}", compiler[0]))?;
        Ok(u8::try_from(status.code().unwrap_or(1)).map_or(ExitCode::FAILURE, ExitCode::from))
    }
}

/// Check the unit in this process and write its record beside `artifact`.
fn record(compiler: &[String], artifact: PathBuf) -> Result<ExitCode, String> {
    let mut callbacks = Recording {
        artifact,
        driver: env::current_exe().ok(),
        result: None,
    };
    let exit = rustc_driver::catch_with_exit_code(|| {
        crate::compat::compiler_entrypoint(compiler, &mut callbacks);
    });
    match callbacks.result {
        Some(Err(error)) => Err(error),
        Some(Ok(())) | None => Ok(exit),
    }
}

/// The compiler callbacks of a recorded unit.
struct Recording {
    artifact: PathBuf,
    /// This driver, a file the unit's fingerprint depends on, so a new
    /// driver checks the unit again.
    driver: Option<PathBuf>,
    result: Option<Result<(), String>>,
}

impl rustc_driver::Callbacks for Recording {
    fn after_crate_root_parsing(
        &mut self,
        compiler: &rustc_interface::interface::Compiler,
        _krate: &mut rustc_ast::Crate,
    ) -> rustc_driver::Compilation {
        // Cargo checks the unit again when the record's roots or this
        // driver change, as it does for clippy's arguments.
        let roots = env::var(record::ROOTS).unwrap_or_default();
        compiler
            .sess
            .env_depinfo
            .lock()
            .insert((Symbol::intern(record::ROOTS), Some(Symbol::intern(&roots))));
        if let Some(driver) = self.driver.as_deref().and_then(Path::to_str) {
            compiler
                .sess
                .file_depinfo
                .lock()
                .insert(Symbol::intern(driver));
        }
        rustc_driver::Compilation::Continue
    }

    fn after_analysis<'tcx>(
        &mut self,
        _compiler: &rustc_interface::interface::Compiler,
        tcx: TyCtxt<'tcx>,
    ) -> rustc_driver::Compilation {
        self.result = Some(write(tcx, &self.artifact));
        rustc_driver::Compilation::Continue
    }
}

fn write(tcx: TyCtxt<'_>, artifact: &Path) -> Result<(), String> {
    let instances = walk::instances(tcx)?;
    record::Record::new(instances)
        .write(artifact)
        .map_err(|error| format!("writing the record beside {}: {error}", artifact.display()))?;
    used::uses(tcx)
        .write(artifact)
        .map_err(|error| format!("writing the uses beside {}: {error}", artifact.display()))
}
