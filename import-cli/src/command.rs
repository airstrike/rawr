//! The `rawr-import` command. `rawr` hands it a program with Rust
//! dependencies in `Cargo.toml`, or native modules, after loading the
//! program through its parse cells; rawr-import reads the cells and parses
//! nothing again. `run` builds the program's runner and starts it, and
//! `check` checks the program against the Rust it imports and runs
//! `cargo check` beside it.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use crate::owner;
use crate::plain::Plain;

const HELP: &str = "rawr-import: runs Rawr programs that use Rust crates\n\n\
`rawr` hands it a program with Rust dependencies in Cargo.toml or a script's\n\
frontmatter, or native modules. The program runs in a runner that links the\n\
rawr crate and the program's own dependencies, built in the project's build\n\
directory beside `cargo build`.\n\n\
Usage:\n  rawr-import [run] [path] [-- args]   Run a program, with its arguments after `--`\n  rawr-import check [path]             Check a program without running it\n  rawr-import cache warm               Build what a first run here builds once per toolchain\n  rawr-import cache clean              Remove what rawr's home holds that no build is using\n\n\
Options:\n  -h, --help       Show this help\n  -V, --version    Show the version";

/// Run `rawr-import` with `arguments`, the executable name excluded.
#[must_use]
pub fn main(arguments: Vec<OsString>) -> ExitCode {
    if arguments
        .first()
        .is_some_and(|argument| argument == rawr_cli::layer::FLAG)
    {
        print!("{}", crate::MANIFEST);
        return ExitCode::SUCCESS;
    }
    // The process `rawr run` starts for a program on the base language.
    if let Some(code) = rawr_cli::supervise::child() {
        return code;
    }
    // A universe runner, as `?play`'s image builds it (#434).
    if std::env::var_os(rawr_import::universe::VARIABLE).is_some_and(|value| value == "1") {
        rawr_import::universe::set();
    }
    let arguments = match rawr_cli::target_arguments(arguments) {
        Ok(arguments) => arguments,
        Err(message) => {
            eprintln!("error: {message}");
            return ExitCode::from(2);
        }
    };
    // What follows `--` is the program's.
    let (arguments, program_arguments) = rawr_cli::split_program_arguments(&arguments);
    let words: Vec<_> = arguments
        .iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect();
    let path = |rest: &[String]| match rest {
        [] => Ok(None),
        [path] if !path.starts_with('-') => Ok(Some(PathBuf::from(path))),
        [option] => Err(format!("unknown argument `{option}`")),
        [_, extra, ..] => Err(format!("unexpected argument `{extra}`")),
    };
    let request = match words.first().map(String::as_str) {
        Some("-h" | "--help" | "help") => {
            println!("{HELP}");
            return ExitCode::SUCCESS;
        }
        Some("-V" | "--version") => {
            println!("rawr-import {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Some("cache") => return cache(&words[1..]),
        Some("run") => path(&words[1..]).map(|path| (false, path)),
        Some("check") if !program_arguments.is_empty() => {
            Err("`check` takes no program arguments".to_owned())
        }
        Some("check") => path(&words[1..]).map(|path| (true, path)),
        _ => path(&words).map(|path| (false, path)),
    };
    match request {
        Ok((check, path)) => program(check, path.as_ref(), &program_arguments),
        Err(message) => {
            eprintln!("error: {message}; run `rawr-import --help` for usage");
            ExitCode::from(2)
        }
    }
}

/// What `rawr cache warm` says when everything was built already.
const WARMED: &str = "rawr: what a first run builds for this toolchain is built; nothing to do";

/// `rawr cache warm`: build what [`warm`] builds. `rawr cache clean`:
/// remove what rawr's home holds per machine and no build is using now
/// (hosts, runtimes, bindings and the nightly check build), as D608.7 lets
/// cleanup do. `cargo clean` removes the rest.
fn cache(words: &[String]) -> ExitCode {
    match words {
        [word] if word == "warm" => return warm(),
        [clean] if clean == "clean" => {}
        [help] if matches!(help.as_str(), "-h" | "--help") => {
            println!("{HELP}");
            return ExitCode::SUCCESS;
        }
        _ => {
            eprintln!("error: `cache` takes `warm` or `clean`; run `rawr-import --help` for usage");
            return ExitCode::from(2);
        }
    }
    let Some(cargo_home) = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")))
    else {
        eprintln!("error: rawr's home needs the environment variable `CARGO_HOME` or `HOME`");
        return ExitCode::FAILURE;
    };
    rawr_import::host::clean(&cargo_home);
    ExitCode::SUCCESS
}

/// `rawr cache warm`: build what a project's first run here builds once per
/// toolchain, printing what that run prints: the nightly that reads crate
/// APIs here (the pinned one outside a project) with its driver and std
/// table, and rawr's runtime for the compiler this directory builds with.
/// `cargo install` has no step that could do it (#1388).
fn warm() -> ExitCode {
    let directory = match std::env::current_dir() {
        Ok(directory) => directory,
        Err(error) => {
            eprintln!("error: the current directory can't be read: {error}");
            return ExitCode::FAILURE;
        }
    };
    let said = rawr_import::progress::said();
    if let Err(message) = crate::project::toolchain(&directory) {
        eprintln!("error: {message}");
        return ExitCode::FAILURE;
    }
    if let Err(error) = rawr_import::runner::runtime::warm(&directory) {
        eprintln!("error: {error}");
        return ExitCode::FAILURE;
    }
    if rawr_import::progress::said() == said {
        eprintln!("{WARMED}");
    }
    ExitCode::SUCCESS
}

fn program(check: bool, path: Option<&PathBuf>, arguments: &[OsString]) -> ExitCode {
    let loaded = match rawr_cli::base::load(path, rawr_cli::base::Build::Program) {
        Ok(loaded) => loaded,
        Err(report) => return failure(&report),
    };
    rawr_cli::cells::keep("rawr-import", &loaded.cells, &loaded.graph);
    if !loaded.graph.declares_rust_import() {
        return rawr_cli::on_base(loaded, check, arguments);
    }
    let rawr_cli::base::Loaded { target, graph, .. } = loaded;
    // rustc checks the crate while Rawr works.
    let cargo = rawr_cli::cargo::begin(&graph);
    if check {
        let cargo = cargo.map(owner::Checking::start);
        rawr_import::progress::begin_phases();
        let checked = owner::check(&target, Some(graph), &Plain);
        rawr_import::progress::phase("checking");
        let cargo = cargo.map(owner::Checking::finish);
        rawr_import::progress::finish(checked.is_ok());
        if rawr_cli::cells::verbose() {
            eprintln!(
                "rawr-import: check rounds {}, {}",
                crate::plain::rounds(),
                rawr_import::progress::work()
            );
            for (index, round) in rawr_import::progress::rounds().iter().enumerate() {
                eprintln!("rawr-import: round {}: {round}", index + 1);
            }
        }
        let checked = owner::settle_checked(checked, cargo.as_ref(), &target, &Plain);
        let checked = checked.as_deref().map_err(Option::as_ref);
        let cargo = cargo.map(|cargo| cargo.status);
        return if rawr_cli::cargo::concluded(checked, cargo) {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    match owner::run_program(&target, Some(graph), &Plain, arguments, cargo) {
        // The program printed its own error as it ran; its status stands.
        Ok(code) => ExitCode::from(u8::try_from(code & 0xff).unwrap_or(1)),
        Err(Some(report)) => failure(&report),
        // Cargo's errors said it all.
        Err(None) => ExitCode::FAILURE,
    }
}

fn failure(report: &rawr::diagnostic::Report) -> ExitCode {
    rawr_cli::diagnostic::emit(report);
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use std::process::ExitCode;

    /// `cache warm` takes no other word, and `cache` names both of its
    /// commands when it gets neither.
    #[test]
    fn cache_takes_warm_or_clean_alone() {
        let words = |words: &[&str]| {
            words
                .iter()
                .map(|word| (*word).to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(super::cache(&words(&["warm", "now"])), ExitCode::from(2));
        assert_eq!(super::cache(&words(&["heat"])), ExitCode::from(2));
        assert!(super::HELP.contains("rawr-import cache warm"));
        assert!(super::WARMED.ends_with("nothing to do"));
    }
}
