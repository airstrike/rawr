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
`rawr` hands it a program with Rust dependencies in Cargo.toml, or native\n\
modules. The program runs in a runner that links the rawr crate and the\n\
program's own dependencies, built in the project's build directory beside\n\
`cargo build`.\n\n\
Usage:\n  rawr-import [run] [path] [-- args]   Run a program, with its arguments after `--`\n  rawr-import check [path]             Check a program without running it\n\n\
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

fn program(check: bool, path: Option<&PathBuf>, arguments: &[OsString]) -> ExitCode {
    let loaded = match rawr_cli::base::load(path) {
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
        let checked = owner::check(&target, Some(graph), &Plain);
        return if rawr_cli::cargo::conclude(checked.as_deref(), cargo) {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    let code = match owner::run_program(&target, Some(graph), &Plain, arguments) {
        // The program printed its own error as it ran; its status stands.
        Ok(code) => ExitCode::from(u8::try_from(code & 0xff).unwrap_or(1)),
        Err(report) => failure(&report),
    };
    rawr_cli::cargo::after_run(cargo);
    code
}

fn failure(report: &rawr::diagnostic::Report) -> ExitCode {
    eprint!("{}", rawr_cli::diagnostic::render(report));
    ExitCode::FAILURE
}
