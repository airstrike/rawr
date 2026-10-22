//! The `rawr` command. It runs Rawr programs on the base language. A
//! program another installed binary serves (rawr-iced, for programs using
//! iced) is loaded here once and handed to that binary, which reads the
//! parse from the program's parse cells instead of parsing it again.
//! Commands `rawr` does not run itself go to the binary that provides them.

#![deny(unsafe_code)]

pub mod base;
pub mod cargo;
pub mod cells;
pub mod diagnostic;
pub mod layer;

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// Run `rawr` with `arguments`, the executable name excluded.
#[must_use]
pub fn main(arguments: Vec<OsString>) -> ExitCode {
    match parse(&arguments) {
        Ok(Request::Help) => {
            println!("{}", help(&layer::installed()));
            ExitCode::SUCCESS
        }
        Ok(Request::Version) => {
            println!("rawr {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Ok(Request::Program { check, path }) => program(check, path.as_ref(), &arguments),
        Ok(Request::Doctor { path }) => doctor(path.as_ref(), &arguments),
        Ok(Request::Other) => forward(&arguments),
        Err(message) => {
            eprintln!("error: {message}; run `rawr --help` for usage");
            ExitCode::from(2)
        }
    }
}

/// What the arguments ask of `rawr` itself.
#[derive(Debug, Eq, PartialEq)]
enum Request {
    Help,
    Version,
    /// `rawr [run] [path]`, or `rawr check [path]`.
    Program { check: bool, path: Option<PathBuf> },
    Doctor { path: Option<PathBuf> },
    /// A command another binary may provide.
    Other,
}

fn parse(arguments: &[OsString]) -> Result<Request, String> {
    let words: Vec<_> = arguments.iter().map(|argument| argument.to_string_lossy()).collect();
    let path = |rest: &[OsString]| match rest {
        [] => Ok(None),
        [path] if !path.to_string_lossy().starts_with('-') => Ok(Some(PathBuf::from(path))),
        [option] => Err(format!("unknown argument `{}`", option.to_string_lossy())),
        [_, extra, ..] => Err(format!("unexpected argument `{}`", extra.to_string_lossy())),
    };
    let asks_help = arguments[1.min(arguments.len())..]
        .iter()
        .any(|argument| argument == "-h" || argument == "--help");
    match words.first().map(AsRef::as_ref) {
        None => Ok(Request::Program {
            check: false,
            path: None,
        }),
        Some("-h" | "--help" | "help") => Ok(Request::Help),
        Some("-V" | "--version") => Ok(Request::Version),
        Some("run" | "check" | "doctor") if asks_help => Ok(Request::Help),
        Some("run") => path(&arguments[1..]).map(|path| Request::Program { check: false, path }),
        // `check` with an option `rawr` does not know is another binary's.
        Some("check") if arguments[1..].iter().any(|argument| argument.to_string_lossy().starts_with('-')) => {
            Ok(Request::Other)
        }
        Some("check") => path(&arguments[1..]).map(|path| Request::Program { check: true, path }),
        Some("doctor") => path(&arguments[1..]).map(|path| Request::Doctor { path }),
        Some(word) if word.starts_with('-') => Err(format!("unknown argument `{word}`")),
        Some(_) if is_path(&arguments[0]) => path(arguments).map(|path| Request::Program { check: false, path }),
        Some(_) => Ok(Request::Other),
    }
}

/// A first argument that names a program rather than a command.
fn is_path(argument: &OsStr) -> bool {
    if argument == "." || argument == ".." || argument == "rawr.toml" {
        return true;
    }
    let extension = Path::new(argument).extension();
    extension == Some(OsStr::new("rawr"))
        || extension == Some(OsStr::new("rs"))
        || argument
            .to_string_lossy()
            .contains(std::path::MAIN_SEPARATOR)
}

fn help(installed: &[layer::Installed]) -> String {
    let mut help = String::from(
        "Rawr: Rust-shaped scripts and apps\n\n`rawr` runs Rawr programs.",
    );
    for known in layer::catalog() {
        help.push_str(&format!(
            " {name} adds support for programs that use {uses}. Install it and `rawr` hands those programs to it.",
            name = known.manifest.name,
            uses = base::uses(&known.manifest.claims),
        ));
    }
    help.push_str(
        "\n\nUsage:\n  rawr [run] [path]    Run a program\n  rawr check [path]    Check a program without running it\n  rawr doctor [path]   Report on the environment and the project\n",
    );
    for binary in installed {
        let words: Vec<_> = binary
            .manifest
            .commands
            .iter()
            .filter(|command| command.word != "doctor")
            .map(ToString::to_string)
            .collect();
        help.push_str(&format!(
            "\nFrom {} ({}):\n  {}\n  Run `rawr <command> --help` for each one.\n",
            binary.manifest.name,
            binary.path.display(),
            words.join("\n  "),
        ));
    }
    for known in layer::catalog() {
        if !installed.iter().any(|binary| binary.manifest.name == known.manifest.name) {
            help.push_str(&format!(
                "\n{} is not installed. Install it with:\n{}\n",
                known.manifest.name,
                layer::install_lines(&known),
            ));
        }
    }
    help.push_str("\nOptions:\n  -h, --help       Show this help\n  -V, --version    Show the version\n\nRAWR_VERBOSE=1 says how the program was loaded and where it went.");
    help
}

/// Load the program once, then run it here or hand it to the binary that
/// serves it.
fn program(check: bool, path: Option<&PathBuf>, arguments: &[OsString]) -> ExitCode {
    let loaded = match base::load(path) {
        Ok(loaded) => loaded,
        Err(report) => return failure(&report),
    };
    let installed = layer::installed();
    let facts = loaded.sheet.facts();
    match layer::claimed(&installed, facts) {
        Err(report) => return failure(&report),
        Ok(Some(binary)) => {
            cells::keep("rawr", &loaded.cells, &loaded.graph);
            if cells::verbose() {
                eprintln!(
                    "rawr: {} serves this program; handing it over",
                    binary.path.display()
                );
            }
            return hand_off(&binary.path, arguments);
        }
        Ok(None) => {}
    }
    cells::tell("rawr", &loaded.cells);
    if let Some(known) = layer::catalog()
        .into_iter()
        .find(|known| known.manifest.claims.claim(facts))
    {
        let report = rawr::diagnostic::Report::error(
            rawr::diagnostic::code::FRAMEWORK_FACADE,
            format!(
                "this program uses {}, which {} serves, and it is not installed",
                known.manifest.claims.reason(facts).unwrap_or_default(),
                known.manifest.name
            ),
        )
        .help(format!(
            "install {}:\n{}",
            known.manifest.name,
            layer::install_lines(&known)
        ))
        .finish();
        return failure(&report);
    }
    // rustc checks a Cargo crate of marked files while Rawr works.
    let cargo = cargo::begin(&loaded.graph);
    if check {
        let checked = base::check(loaded);
        return if cargo::conclude(checked.as_deref(), cargo) {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    let code = match base::run(loaded) {
        Ok(None) => ExitCode::SUCCESS,
        Ok(Some(error)) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
        Err(base::Failed::Compile(report)) => failure(&report),
        Err(base::Failed::Panic(report)) => {
            eprint!("{}", diagnostic::render(&report));
            // Rust's exit status for a panic.
            ExitCode::from(101)
        }
    };
    cargo::after_run(cargo);
    code
}

/// `rawr doctor`: what `rawr` knows, then each installed binary's own report.
fn doctor(path: Option<&PathBuf>, arguments: &[OsString]) -> ExitCode {
    let installed = layer::installed();
    println!("rawr {}", env!("CARGO_PKG_VERSION"));
    if let Ok(executable) = std::env::current_exe() {
        println!("  at {}", executable.display());
    }
    for known in layer::catalog() {
        match installed
            .iter()
            .find(|binary| binary.manifest.name == known.manifest.name)
        {
            Some(binary) => println!("{}: {}", known.manifest.name, binary.path.display()),
            None => println!(
                "{}: not installed; programs using {} need it. Install it with:\n{}",
                known.manifest.name,
                base::uses(&known.manifest.claims),
                layer::install_lines(&known)
            ),
        }
    }
    match base::discover(path) {
        Ok(target) => println!("program: {}", target.source().path().display()),
        Err(report) => {
            println!("program: none found");
            if path.is_some() {
                return failure(&report);
            }
        }
    }
    let mut code = ExitCode::SUCCESS;
    for binary in installed
        .iter()
        .filter(|binary| binary.manifest.provides_word("doctor"))
    {
        println!();
        match Command::new(&binary.path).args(arguments).status() {
            Ok(status) if status.success() => {}
            Ok(_) => code = ExitCode::FAILURE,
            Err(error) => {
                eprintln!("error: cannot run {}: {error}", binary.path.display());
                code = ExitCode::FAILURE;
            }
        }
    }
    code
}

/// A command `rawr` does not run itself: the installed binary providing it
/// runs it.
fn forward(arguments: &[OsString]) -> ExitCode {
    if let Some(binary) = layer::installed()
        .into_iter()
        .find(|binary| binary.manifest.provides(arguments))
    {
        return hand_off(&binary.path, arguments);
    }
    for known in layer::catalog() {
        if let Some(command) = known.manifest.provided(arguments) {
            eprintln!(
                "error: `{command}` comes with {}, which is not installed. Install it with:\n{}",
                known.manifest.name,
                layer::install_lines(&known)
            );
            return ExitCode::FAILURE;
        }
    }
    let command = arguments
        .first()
        .map(|word| word.to_string_lossy().into_owned())
        .unwrap_or_default();
    eprintln!("error: unknown command `{command}`; run `rawr --help` for usage");
    ExitCode::from(2)
}

/// Run `binary` with the same arguments in place of this process.
fn hand_off(binary: &Path, arguments: &[OsString]) -> ExitCode {
    let mut command = Command::new(binary);
    command.args(arguments);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        let error = command.exec();
        eprintln!("error: cannot run {}: {error}", binary.display());
        ExitCode::FAILURE
    }
    #[cfg(not(unix))]
    match command.status() {
        Ok(status) => ExitCode::from(status.code().map_or(1, |code| code as u8)),
        Err(error) => {
            eprintln!("error: cannot run {}: {error}", binary.display());
            ExitCode::FAILURE
        }
    }
}

fn failure(report: &rawr::diagnostic::Report) -> ExitCode {
    eprint!("{}", diagnostic::render(report));
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::{parse, Request};
    use std::ffi::OsString;
    use std::path::PathBuf;

    fn request(words: &[&str]) -> Result<Request, String> {
        parse(&words.iter().map(OsString::from).collect::<Vec<_>>())
    }

    #[test]
    fn rawr_runs_and_checks_programs_and_leaves_other_commands_to_others() {
        let program = |check, path: Option<&str>| {
            Ok(Request::Program {
                check,
                path: path.map(PathBuf::from),
            })
        };
        assert_eq!(request(&[]), program(false, None));
        assert_eq!(request(&["run"]), program(false, None));
        assert_eq!(request(&["run", "app"]), program(false, Some("app")));
        assert_eq!(request(&["cart.rawr"]), program(false, Some("cart.rawr")));
        assert_eq!(request(&["check", "."]), program(true, Some(".")));
        assert_eq!(request(&["check", "--as-rust"]), Ok(Request::Other));
        assert_eq!(request(&["preview", "x"]), Ok(Request::Other));
        assert_eq!(request(&["new", "hello"]), Ok(Request::Other));
        assert_eq!(request(&["run", "--help"]), Ok(Request::Help));
        assert_eq!(request(&["--version"]), Ok(Request::Version));
        assert!(request(&["run", "a", "b"]).is_err());
        assert!(request(&["--bogus"]).is_err());
    }
}
