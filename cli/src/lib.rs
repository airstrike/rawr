//! The `rawr` command. It runs Rawr programs on the base language. A
//! program an installed extension serves (a `rawr-<name>` binary whose
//! manifest claims it) is loaded here once and handed to that binary, which
//! reads the parse from the program's parse cells instead of parsing it
//! again. Commands `rawr` does not run itself go to the binary that provides
//! them. `rawr` knows extensions only as data.

#![deny(unsafe_code)]

pub mod base;
pub mod cargo;
pub mod cells;
pub mod diagnostic;
pub mod layer;
pub mod new;

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
        Ok(Request::New(New::Help)) => {
            println!("{}", new_help(&layer::installed()));
            ExitCode::SUCCESS
        }
        Ok(Request::New(New::Create { path, template })) => {
            create(path, template.as_deref(), &arguments)
        }
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
    Program {
        check: bool,
        path: Option<PathBuf>,
    },
    Doctor {
        path: Option<PathBuf>,
    },
    /// `rawr new`.
    New(New),
    /// A command another binary may provide.
    Other,
}

/// What `rawr new` asks for.
#[derive(Debug, Eq, PartialEq)]
enum New {
    Help,
    /// A crate at `path`, plain or from an installed binary's `template`.
    Create {
        path: PathBuf,
        template: Option<String>,
    },
}

fn parse_new(arguments: &[OsString]) -> Result<New, String> {
    let mut path = None;
    let mut template = None;
    let mut arguments = arguments.iter().map(|argument| argument.to_string_lossy());
    while let Some(argument) = arguments.next() {
        match argument.as_ref() {
            "-h" | "--help" => return Ok(New::Help),
            "--template" => match arguments.next() {
                Some(name) if !name.starts_with('-') => template = Some(name.into_owned()),
                _ => return Err("`--template` needs a template name".to_owned()),
            },
            option if option.starts_with('-') => {
                return Err(format!("unknown argument `{option}`"));
            }
            _ if path.is_some() => return Err(format!("unexpected argument `{argument}`")),
            _ => path = Some(PathBuf::from(argument.as_ref())),
        }
    }
    let path = path.ok_or("`rawr new` needs the path of the crate to create")?;
    Ok(New::Create { path, template })
}

fn parse(arguments: &[OsString]) -> Result<Request, String> {
    let words: Vec<_> = arguments
        .iter()
        .map(|argument| argument.to_string_lossy())
        .collect();
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
        Some("check")
            if arguments[1..]
                .iter()
                .any(|argument| argument.to_string_lossy().starts_with('-')) =>
        {
            Ok(Request::Other)
        }
        Some("check") => path(&arguments[1..]).map(|path| Request::Program { check: true, path }),
        Some("doctor") => path(&arguments[1..]).map(|path| Request::Doctor { path }),
        Some("new") => parse_new(&arguments[1..]).map(Request::New),
        Some(word) if word.starts_with('-') => Err(format!("unknown argument `{word}`")),
        Some(_) if is_path(&arguments[0]) => {
            path(arguments).map(|path| Request::Program { check: false, path })
        }
        Some(_) => Ok(Request::Other),
    }
}

/// A first argument that names a program rather than a command.
fn is_path(argument: &OsStr) -> bool {
    // `rawr.toml` and `.rawr` count as paths only so discovery can say how
    // to convert them.
    if argument == "." || argument == ".." || argument == "Cargo.toml" || argument == "rawr.toml" {
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
        "Rawr: a Rust interpreter for your dev builds\n\n\
         `rawr` runs a Cargo crate whose files start with `// rawr`, or one `.rs` file. \
         Installed extensions serve more programs and add commands: `rawr` hands a \
         program to the extension that serves it.\n\n\
         Usage:\n  rawr [run] [path]    Run a program\n  rawr check [path]    Check a program without running it\n  \
         rawr new <path>      Create a crate with a marked `src/main.rs`\n  rawr doctor [path]   Report on the environment and the project\n",
    );
    for binary in installed {
        help.push_str(&format!(
            "\nFrom {} ({}):\n  programs that use {}\n",
            binary.manifest.name,
            binary.path.display(),
            base::uses(&binary.manifest.claims),
        ));
        let words: Vec<_> = binary
            .manifest
            .commands
            .iter()
            .filter(|command| command.word != "doctor")
            .map(ToString::to_string)
            .collect();
        if !words.is_empty() {
            help.push_str(&format!(
                "  {}\n  Run `rawr <command> --help` for each one.\n",
                words.join("\n  ")
            ));
        }
        if !binary.manifest.templates.is_empty() {
            help.push_str("  `rawr new <path> --template <name>`: see `rawr new --help`\n");
        }
    }
    for known in layer::catalog() {
        if !installed
            .iter()
            .any(|binary| binary.manifest.name == known.manifest.name)
        {
            help.push_str(&format!(
                "\n{} is not installed. It serves programs that use {}. Install it with:\n{}\n",
                known.manifest.name,
                base::uses(&known.manifest.claims),
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
    if let Some(known) = layer::known_claimant(facts) {
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
    on_base(loaded, check)
}

/// Run or check a loaded program on the base language: its top level,
/// then `fn main` if it has one, as in Rust. `cargo check` runs beside it
/// for a Cargo crate.
#[must_use]
pub fn on_base(loaded: base::Loaded, check: bool) -> ExitCode {
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

/// `rawr new --help`: the usage and the templates installed binaries offer.
fn new_help(installed: &[layer::Installed]) -> String {
    let mut help = String::from(
        "Create a Cargo crate whose `src/main.rs` starts with `// rawr`.\n\n\
         Usage:\n  rawr new <path>                     A program with no dependencies\n  \
         rawr new <path> --template <name>   A template an installed binary offers\n",
    );
    let offered: Vec<_> = installed
        .iter()
        .flat_map(|binary| {
            binary
                .manifest
                .templates
                .iter()
                .map(move |template| format!("  {template:<12} from {}", binary.manifest.name))
        })
        .collect();
    if !offered.is_empty() {
        help.push_str(&format!("\nTemplates:\n{}\n", offered.join("\n")));
    }
    for known in layer::catalog() {
        if known.manifest.templates.is_empty()
            || installed
                .iter()
                .any(|binary| binary.manifest.name == known.manifest.name)
        {
            continue;
        }
        help.push_str(&format!(
            "\n{} offers {}. Install it with:\n{}\n",
            known.manifest.name,
            known
                .manifest
                .templates
                .iter()
                .map(|template| format!("`{template}`"))
                .collect::<Vec<_>>()
                .join(", "),
            layer::install_lines(&known),
        ));
    }
    help
}

/// `rawr new`: a plain crate, or the template's crate written by the
/// installed binary that offers it.
fn create(path: PathBuf, template: Option<&str>, arguments: &[OsString]) -> ExitCode {
    let Some(template) = template else {
        return match new::create(path, new::plain) {
            Ok(created) => {
                println!(
                    "Created a Rawr crate at {root}\n\nNext:\n  cd {root}\n  rawr run",
                    root = created.root().display()
                );
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("error: {error}");
                ExitCode::FAILURE
            }
        };
    };
    let offers =
        |manifest: &layer::Manifest| manifest.templates.iter().any(|name| name == template);
    let installed = layer::installed();
    if let Some(binary) = installed.iter().find(|binary| offers(&binary.manifest)) {
        return hand_off(&binary.path, arguments);
    }
    if let Some(known) = layer::catalog()
        .into_iter()
        .find(|known| offers(&known.manifest))
    {
        eprintln!(
            "error: the `{template}` template comes with {}, which is not installed. Install it with:\n{}",
            known.manifest.name,
            layer::install_lines(&known)
        );
        return ExitCode::FAILURE;
    }
    eprintln!(
        "error: no installed binary offers a template named `{template}`; run `rawr new --help` for the templates"
    );
    ExitCode::from(2)
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
        assert_eq!(request(&["cart.rs"]), program(false, Some("cart.rs")));
        assert_eq!(request(&["Cargo.toml"]), program(false, Some("Cargo.toml")));
        assert_eq!(request(&["cart.rawr"]), program(false, Some("cart.rawr")));
        assert_eq!(request(&["check", "."]), program(true, Some(".")));
        assert_eq!(request(&["check", "--verbose"]), Ok(Request::Other));
        assert_eq!(request(&["preview", "x"]), Ok(Request::Other));
        assert_eq!(
            request(&["new", "hello"]),
            Ok(Request::New(super::New::Create {
                path: PathBuf::from("hello"),
                template: None,
            }))
        );
        assert_eq!(
            request(&["new", "--template", "shapes", "hello"]),
            Ok(Request::New(super::New::Create {
                path: PathBuf::from("hello"),
                template: Some("shapes".to_owned()),
            }))
        );
        assert_eq!(
            request(&["new", "hello", "--help"]),
            Ok(Request::New(super::New::Help))
        );
        assert!(request(&["new"]).is_err());
        assert!(request(&["new", "hello", "--template"]).is_err());
        assert_eq!(request(&["run", "--help"]), Ok(Request::Help));
        assert_eq!(request(&["--version"]), Ok(Request::Version));
        assert!(request(&["run", "a", "b"]).is_err());
        assert!(request(&["--bogus"]).is_err());
    }
}
