//! `rawr run` of a program on the base language (D500.1). The program runs
//! in a process of its own, which `rawr` starts again as itself. That
//! process sees the arguments after `--` as `std::env::args()`, the first
//! one being the program's name as `cargo run` gives it, and it inherits
//! stdin, stdout and stderr. `rawr` watches the program's files meanwhile:
//! a save that still compiles restarts a program that is running, and
//! runs again one that ended when stdin is a terminal. Without a terminal
//! the command ends with the program, with its exit status.

use std::ffi::{OsStr, OsString};
use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, ExitStatus};
use std::time::{Duration, Instant};

use crate::base;

/// The variable that tells a `rawr` process to run the program at its path
/// once, as the process [`run`] starts.
const PROGRAM: &str = "RAWR_RUN_PROGRAM";

const DEBOUNCE: Duration = Duration::from_millis(30);
const POLL: Duration = Duration::from_millis(25);

/// In the process [`run`] starts: run the program once and give its exit
/// status. `None` in any other process.
#[must_use]
pub fn child() -> Option<ExitCode> {
    let path = PathBuf::from(std::env::var_os(PROGRAM)?);
    Some(match base::load(Some(&path)) {
        Ok(loaded) => crate::run_once(loaded),
        Err(report) => crate::failure(&report),
    })
}

/// The name a program sees as its first argument: the path of the binary
/// `cargo run` builds, `<target>/debug/<name>` in the crate's Cargo target
/// directory (the workspace's, for a member), or the
/// file's path without `.rs` for a single file.
#[must_use]
pub fn program_name(target: &rawr::project::Target) -> OsString {
    let Some(project) = target.project() else {
        return target.source().path().with_extension("").into_os_string();
    };
    let root = project.root().as_path();
    let name = package_name(&root.join("Cargo.toml")).unwrap_or_else(|| {
        root.file_name().map_or_else(
            || "main".to_owned(),
            |name| name.to_string_lossy().into_owned(),
        )
    });
    crate::cells::target_directory(root)
        .join("debug")
        .join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
        .into_os_string()
}

/// The binary name a `Cargo.toml` gives: its first `[[bin]]`'s name, else
/// its package's name.
fn package_name(manifest: &Path) -> Option<String> {
    let text = std::fs::read_to_string(manifest).ok()?;
    let document = text.parse::<toml_edit::DocumentMut>().ok()?;
    let bin = document
        .get("bin")
        .and_then(|bin| bin.as_array_of_tables())
        .and_then(|bins| bins.iter().next())
        .and_then(|bin| bin.get("name"))
        .and_then(|name| name.as_str());
    bin.or_else(|| document.get("package")?.get("name")?.as_str())
        .map(str::to_owned)
}

/// The contents of the files `graph` loaded: the program's sources, its
/// native modules and its manifests. A save changes them; a build that
/// touches a file without changing it doesn't.
#[must_use]
pub fn contents(graph: &rawr::package::Graph) -> Vec<(PathBuf, Option<Vec<u8>>)> {
    let source = rawr::reload::source::Graph::from_package(graph);
    rawr::reload::source::Inputs::from_graph(&source)
        .paths()
        .iter()
        .map(|path| (path.clone(), std::fs::read(path).ok()))
        .collect()
}

/// Where the program is, for the process that runs it: the crate's root,
/// or the single file.
fn location(target: &rawr::project::Target) -> PathBuf {
    target.project().map_or_else(
        || target.source().path().to_path_buf(),
        |project| project.root().as_path().to_path_buf(),
    )
}

/// Whether a program that ended runs again on a save: when someone at a
/// terminal can give it input again, or a `rawr mcp` supervises the run.
#[must_use]
pub fn interactive() -> bool {
    std::io::stdin().is_terminal() || crate::events::supervised()
}

/// The exit status a command gives for a program's `status`.
#[must_use]
pub fn exit_code(status: ExitStatus) -> ExitCode {
    if let Some(code) = status.code() {
        return ExitCode::from(u8::try_from(code & 0xff).unwrap_or(1));
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        if let Some(signal) = status.signal() {
            return ExitCode::from(u8::try_from(128 + signal).unwrap_or(1));
        }
    }
    ExitCode::FAILURE
}

/// Run the loaded program in its own process with `arguments`, restart it
/// on each save that compiles, and run it again after it ends when
/// [`interactive`]. `cargo`'s errors are printed when the program first
/// ends.
#[must_use]
pub fn run(
    loaded: base::Loaded,
    arguments: &[OsString],
    mut cargo: Option<crate::cargo::Pending>,
) -> ExitCode {
    let location = location(&loaded.target);
    let name = program_name(&loaded.target);
    let mut watcher = Watcher::open(&loaded.graph);
    let mut started_from = contents(&loaded.graph);
    drop(loaded);
    // The program shares this process's group, which a dead `rawr mcp`'s
    // watch ends.
    crate::events::watch_parent(|| {});
    let mut child = match start(&location, &name, arguments) {
        Ok(child) => Some(child),
        Err(error) => {
            eprintln!("error: cannot start the program: {error}");
            return ExitCode::FAILURE;
        }
    };
    crate::events::running();
    let interactive = interactive();
    loop {
        if let Some(running) = child.as_mut() {
            match running.try_wait() {
                Ok(Some(status)) => {
                    child = None;
                    crate::events::exited(status);
                    crate::cargo::after_run(cargo.take());
                    if !interactive || watcher.is_none() {
                        return exit_code(status);
                    }
                    eprintln!("rawr: the program ended; a save runs it again");
                }
                Ok(None) => {}
                Err(error) => {
                    eprintln!("error: cannot wait for the program: {error}");
                    return ExitCode::FAILURE;
                }
            }
        }
        let Some(watching) = watcher.as_ref() else {
            // Nothing to watch: wait for the program.
            std::thread::sleep(POLL);
            continue;
        };
        if !watching.changed() {
            continue;
        }
        // A save: load the program again, and keep the one running unless
        // the new one compiles.
        let loaded = match base::load(Some(&location)) {
            Ok(loaded) => loaded,
            Err(report) => {
                eprint!("{}", crate::diagnostic::render(&report));
                crate::events::failed(&report);
                continue;
            }
        };
        let now = contents(&loaded.graph);
        if now == started_from {
            continue;
        }
        // Each change is acted on, or reported, once.
        started_from = now;
        if let Some(next) = Watcher::open(&loaded.graph) {
            watcher = Some(next);
        }
        if let Ok(Some(binary)) =
            crate::layer::claimed(&crate::layer::installed(), loaded.sheet.facts())
        {
            eprintln!(
                "rawr: {} serves this program now; stop it and run it again with `rawr run`",
                binary.manifest.name
            );
            continue;
        }
        if let Err(report) = base::check(loaded) {
            eprint!("{}", crate::diagnostic::render(&report));
            crate::events::failed(&report);
            continue;
        }
        if let Some(mut running) = child.take() {
            eprintln!("rawr: the source changed; restarting the program");
            crate::events::restarting("the source changed");
            let _ = running.kill();
            let _ = running.wait();
        }
        child = match start(&location, &name, arguments) {
            Ok(child) => Some(child),
            Err(error) => {
                eprintln!("error: cannot start the program: {error}");
                return ExitCode::FAILURE;
            }
        };
        crate::events::running();
    }
}

/// This executable again, running the program at `location` once, with
/// `name` and `arguments` as its arguments.
fn start(location: &Path, name: &OsStr, arguments: &[OsString]) -> std::io::Result<Child> {
    let mut command = Command::new(std::env::current_exe()?);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.arg0(name);
    }
    #[cfg(not(unix))]
    let _ = name;
    command
        .args(arguments)
        .env(PROGRAM, location)
        .env_remove(crate::events::EVENTS)
        .env_remove(crate::events::PARENT)
        .spawn()
}

/// The program's files, watched for saves.
pub struct Watcher(rawr_watch::Adapter);

impl Watcher {
    /// A watcher over `graph`'s files, or `None` when the platform's
    /// watcher can't start: the program then runs once.
    #[must_use]
    pub fn open(graph: &rawr::package::Graph) -> Option<Self> {
        let source = rawr::reload::source::Graph::from_package(graph);
        rawr_watch::Adapter::open(&source, DEBOUNCE)
            .map(Self)
            .map_err(|error| eprintln!("rawr: saves won't restart the program: {error}"))
            .ok()
    }

    /// Wait for a save, past its debounce.
    pub fn wait(&self) {
        while !self.changed() {}
    }

    /// Wait up to one poll for a save, past its debounce.
    fn changed(&self) -> bool {
        let now = Instant::now();
        if self.0.start(now).is_some() {
            return true;
        }
        let timeout = self
            .0
            .deadline()
            .map_or(POLL, |deadline| deadline.saturating_duration_since(now))
            .min(POLL);
        let _ = self.0.notices().recv_timeout(timeout);
        false
    }
}
