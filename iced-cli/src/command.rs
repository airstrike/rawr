//! Deterministic command-line grammar independent of filesystem state.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};

/// Concise CLI help text.
pub const HELP: &str = "rawr-iced: iced support for the rawr command\n\n`rawr` hands programs that use iced to rawr-iced, which opens them and\nreloads them when you save. rawr-iced also runs the commands below that\n`rawr` does not run itself.\n\nUsage:\n  rawr [run] [path]\n  rawr preview [path] [name]\n  rawr snapshot --check [--verbose] [path]\n  rawr snapshot --update [--verbose] [path]\n  rawr check [path]\n  rawr new <path> --template iced\n  rawr doctor [path]\n  rawr cache clean [--all]\n\n`rawr mcp` runs rawr-iced's agent tools as `rawr-iced mcp call <tool>`.\n\nOptions:\n  -h, --help       Show this help\n  -V, --version    Show the CLI version\n      --verbose    Show snapshot storage details";

/// Deterministic snapshot operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    /// Compare fresh artifacts without changing committed baselines.
    Check,
    /// Transactionally replace the complete committed baseline generation.
    Update,
}

/// Parsed work requested by the user.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    /// Check, prepare, and open one native Rawr application.
    Run {
        /// Explicit source or project path, or current-directory discovery.
        path: Option<PathBuf>,
    },
    /// Check and open the preview host straight into one preview.
    Preview {
        /// Explicit source or project path, or current-directory discovery.
        path: Option<PathBuf>,
        /// Preview to open (`name` or `group/name`); a sole argument that is
        /// not an existing path is this. Otherwise the last one picked.
        name: Option<String>,
    },
    /// Render and check or update deterministic preview snapshots.
    Snapshot {
        /// Baseline operation.
        mode: Mode,
        /// Include absolute paths and storage details.
        verbose: bool,
        /// Explicit source or project path, or current-directory discovery.
        path: Option<PathBuf>,
    },
    /// Check one application without opening a native window.
    Check {
        /// Explicit source or project path, or current-directory discovery.
        path: Option<PathBuf>,
    },
    /// Create a starter Cargo crate.
    New {
        /// Destination directory.
        path: PathBuf,
    },
    /// Run one of rawr-iced's agent tools for `rawr mcp`: the call on
    /// stdin, the result on stdout (`rawr_cli::mcp::content`).
    McpCall {
        /// The tool.
        tool: String,
    },
    /// Report environment and project health.
    Doctor {
        /// Explicit source or project path, or current-directory discovery.
        path: Option<PathBuf>,
    },
    /// Evict every shared-store build no running process uses.
    CacheClean {
        /// Also remove the current project's `<target>/rawr`.
        all: bool,
    },
}

/// Parser result that may terminate without executing a command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Request {
    /// Execute one parsed command.
    Execute(Command),
    /// Print [`HELP`] successfully.
    Help,
    /// Print the CLI package version successfully.
    Version,
}

/// Invalid command-line input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    /// A plain first word did not name a known command.
    UnknownCommand(OsString),
    /// An option was not recognized in this position.
    UnknownArgument(OsString),
    /// A command did not receive its required positional argument.
    MissingArgument {
        /// Command that requires the argument.
        command: &'static str,
        /// Human-readable argument spelling.
        argument: &'static str,
    },
    /// A command received more positional arguments than its grammar permits.
    ExtraArgument(OsString),
}

/// Parse arguments after the executable name.
pub fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Request, Error> {
    let mut arguments = arguments.into_iter();
    let Some(first) = arguments.next() else {
        return Ok(Request::Execute(Command::Run { path: None }));
    };

    match first.to_str() {
        Some("-h" | "--help" | "help") => terminal(Request::Help, &mut arguments),
        Some("-V" | "--version") => terminal(Request::Version, &mut arguments),
        Some("run") => optional_path(&mut arguments, |path| Command::Run { path }),
        Some("preview") => preview(&mut arguments),
        Some("snapshot") => snapshot(&mut arguments),
        Some("check") => optional_path(&mut arguments, |path| Command::Check { path }),
        Some("doctor") => optional_path(&mut arguments, |path| Command::Doctor { path }),
        Some("mcp") => match (arguments.next(), arguments.next()) {
            (Some(call), Some(tool)) if call == "call" => terminal(
                Request::Execute(Command::McpCall {
                    tool: tool.to_string_lossy().into_owned(),
                }),
                &mut arguments,
            ),
            (Some(help), _) if help == "-h" || help == "--help" => {
                terminal(Request::Help, &mut arguments)
            }
            (Some(call), None) if call == "call" => Err(Error::MissingArgument {
                command: "mcp call",
                argument: "a tool name",
            }),
            (Some(other), _) => Err(Error::UnknownArgument(other)),
            (None, _) => Err(Error::MissingArgument {
                command: "mcp",
                argument: "call <tool>",
            }),
        },
        Some("new") => new(&mut arguments),
        Some("cache") => match (arguments.next(), arguments.next()) {
            (Some(clean), all)
                if clean == "clean" && all.as_ref().is_none_or(|all| all == "--all") =>
            {
                terminal(
                    Request::Execute(Command::CacheClean { all: all.is_some() }),
                    &mut arguments,
                )
            }
            (Some(clean), Some(other)) if clean == "clean" => Err(Error::UnknownArgument(other)),
            (Some(other), _) => Err(Error::UnknownArgument(other)),
            (None, _) => Err(Error::MissingArgument {
                command: "cache",
                argument: "clean",
            }),
        },
        Some(word) if word.starts_with('-') => Err(Error::UnknownArgument(first)),
        _ if is_implicit_run_path(&first) => terminal(
            Request::Execute(Command::Run {
                path: Some(PathBuf::from(first)),
            }),
            &mut arguments,
        ),
        _ => Err(Error::UnknownCommand(first)),
    }
}

fn preview(arguments: &mut impl Iterator<Item = OsString>) -> Result<Request, Error> {
    let mut positional = Vec::new();
    for argument in arguments.by_ref() {
        if argument == "-h" || argument == "--help" {
            return terminal(Request::Help, arguments);
        }
        if argument.to_string_lossy().starts_with('-') {
            return Err(Error::UnknownArgument(argument));
        }
        if positional.len() == 2 {
            return Err(Error::ExtraArgument(argument));
        }
        positional.push(argument);
    }
    let mut positional = positional.into_iter();
    let path = positional.next().map(PathBuf::from);
    let name = positional
        .next()
        .map(|name| name.to_string_lossy().into_owned());
    Ok(Request::Execute(Command::Preview { path, name }))
}

fn snapshot(arguments: &mut impl Iterator<Item = OsString>) -> Result<Request, Error> {
    let Some(argument) = arguments.next() else {
        return Err(Error::MissingArgument {
            command: "snapshot",
            argument: "--check or --update",
        });
    };
    match argument.to_str() {
        Some("-h" | "--help") => terminal(Request::Help, arguments),
        Some("--check") => snapshot_path(Mode::Check, arguments),
        Some("--update") => snapshot_path(Mode::Update, arguments),
        _ => Err(Error::UnknownArgument(argument)),
    }
}

fn snapshot_path(
    mode: Mode,
    arguments: &mut impl Iterator<Item = OsString>,
) -> Result<Request, Error> {
    let Some(argument) = arguments.next() else {
        return Ok(Request::Execute(Command::Snapshot {
            mode,
            verbose: false,
            path: None,
        }));
    };
    if argument == "-h" || argument == "--help" {
        return terminal(Request::Help, arguments);
    }
    if argument == "--verbose" {
        return optional_path(arguments, |path| Command::Snapshot {
            mode,
            verbose: true,
            path,
        });
    }
    if argument.to_string_lossy().starts_with('-') {
        return Err(Error::UnknownArgument(argument));
    }
    terminal(
        Request::Execute(Command::Snapshot {
            mode,
            verbose: false,
            path: Some(PathBuf::from(argument)),
        }),
        arguments,
    )
}

fn terminal(
    request: Request,
    arguments: &mut impl Iterator<Item = OsString>,
) -> Result<Request, Error> {
    match arguments.next() {
        Some(extra) => Err(Error::ExtraArgument(extra)),
        None => Ok(request),
    }
}

fn optional_path(
    arguments: &mut impl Iterator<Item = OsString>,
    command: impl FnOnce(Option<PathBuf>) -> Command,
) -> Result<Request, Error> {
    let Some(argument) = arguments.next() else {
        return Ok(Request::Execute(command(None)));
    };
    if argument == "-h" || argument == "--help" {
        return terminal(Request::Help, arguments);
    }
    if argument.to_string_lossy().starts_with('-') {
        return Err(Error::UnknownArgument(argument));
    }
    let path = PathBuf::from(argument);
    match arguments.next() {
        Some(extra) => Err(Error::ExtraArgument(extra)),
        None => Ok(Request::Execute(command(Some(path)))),
    }
}

/// `new <path> [--template iced]`: rawr-iced writes its one template,
/// `iced`, which `rawr new --template iced` hands to it.
fn new(arguments: &mut impl Iterator<Item = OsString>) -> Result<Request, Error> {
    let mut path = None;
    while let Some(argument) = arguments.next() {
        if argument == "-h" || argument == "--help" {
            return terminal(Request::Help, arguments);
        }
        if argument == "--template" {
            match arguments.next() {
                Some(name) if name == crate::new::TEMPLATE => continue,
                Some(name) => return Err(Error::UnknownArgument(name)),
                None => {
                    return Err(Error::MissingArgument {
                        command: "new --template",
                        argument: crate::new::TEMPLATE,
                    });
                }
            }
        }
        if argument.to_string_lossy().starts_with('-') {
            return Err(Error::UnknownArgument(argument));
        }
        if path.is_some() {
            return Err(Error::ExtraArgument(argument));
        }
        path = Some(PathBuf::from(argument));
    }
    let path = path.ok_or(Error::MissingArgument {
        command: "new",
        argument: "<path>",
    })?;
    Ok(Request::Execute(Command::New { path }))
}

fn is_implicit_run_path(argument: &OsStr) -> bool {
    // `rawr.toml` and `.rawr` count as paths only so discovery can say how
    // to convert them.
    if argument == "." || argument == ".." || argument == "Cargo.toml" || argument == "rawr.toml" {
        return true;
    }
    let path = Path::new(argument);
    path.extension() == Some(OsStr::new("rs"))
        || path.extension() == Some(OsStr::new("rawr"))
        || argument
            .to_string_lossy()
            .contains(std::path::MAIN_SEPARATOR)
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownCommand(command) => write!(
                formatter,
                "unknown command `{}`; run `rawr --help` for usage",
                command.to_string_lossy()
            ),
            Self::UnknownArgument(argument) => write!(
                formatter,
                "unknown argument `{}`; run `rawr --help` for usage",
                argument.to_string_lossy()
            ),
            Self::MissingArgument { command, argument } => write!(
                formatter,
                "`rawr {command}` requires {argument}; run `rawr --help` for usage"
            ),
            Self::ExtraArgument(argument) => write!(
                formatter,
                "unexpected extra argument `{}`; run `rawr --help` for usage",
                argument.to_string_lossy()
            ),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::{Command, Error, Request};

    fn parse(arguments: &[&str]) -> Result<Request, Error> {
        super::parse(arguments.iter().map(|argument| OsString::from(*argument)))
    }

    #[test]
    fn bare_command_and_run_alias_dispatch_identically() {
        assert_eq!(parse(&[]), parse(&["run"]));
        assert_eq!(
            parse(&[]).unwrap(),
            Request::Execute(Command::Run { path: None })
        );
    }

    #[test]
    fn unambiguous_path_tokens_are_implicit_run_targets() {
        for path in [
            ".",
            "..",
            "./counter",
            "src/main.rs",
            "Cargo.toml",
            "rawr.toml",
        ] {
            assert_eq!(
                parse(&[path]).unwrap(),
                Request::Execute(Command::Run {
                    path: Some(PathBuf::from(path))
                })
            );
        }
    }

    #[test]
    fn explicit_commands_accept_plain_paths() {
        assert_eq!(
            parse(&["check", "counter"]).unwrap(),
            Request::Execute(Command::Check {
                path: Some(PathBuf::from("counter"))
            })
        );
        assert_eq!(
            parse(&["preview", "counter"]).unwrap(),
            Request::Execute(Command::Preview {
                path: Some(PathBuf::from("counter")),
                name: None
            })
        );
        assert_eq!(
            parse(&["preview", ".", "kit/dial"]).unwrap(),
            Request::Execute(Command::Preview {
                path: Some(PathBuf::from(".")),
                name: Some("kit/dial".to_owned())
            })
        );
        assert_eq!(
            parse(&["preview", ".", "dial", "extra"]).unwrap_err(),
            Error::ExtraArgument(OsString::from("extra"))
        );
        assert_eq!(
            parse(&["new", "counter"]).unwrap(),
            Request::Execute(Command::New {
                path: PathBuf::from("counter")
            })
        );
        assert_eq!(
            parse(&["new", "--template", "iced", "counter"]).unwrap(),
            Request::Execute(Command::New {
                path: PathBuf::from("counter")
            })
        );
        assert_eq!(
            parse(&["new", "counter", "--template", "tui"]).unwrap_err(),
            Error::UnknownArgument(OsString::from("tui"))
        );
        assert_eq!(
            parse(&["doctor", "counter"]).unwrap(),
            Request::Execute(Command::Doctor {
                path: Some(PathBuf::from("counter"))
            })
        );
        assert_eq!(
            parse(&["snapshot", "--check", "counter"]).unwrap(),
            Request::Execute(Command::Snapshot {
                mode: super::Mode::Check,
                verbose: false,
                path: Some(PathBuf::from("counter"))
            })
        );
        assert_eq!(
            parse(&["snapshot", "--update", "--verbose"]).unwrap(),
            Request::Execute(Command::Snapshot {
                mode: super::Mode::Update,
                verbose: true,
                path: None
            })
        );
    }

    #[test]
    fn help_and_version_aliases_terminate_successfully() {
        for arguments in [
            ["-h"].as_slice(),
            ["--help"].as_slice(),
            ["help"].as_slice(),
        ] {
            assert_eq!(parse(arguments).unwrap(), Request::Help);
        }
        for arguments in [["-V"].as_slice(), ["--version"].as_slice()] {
            assert_eq!(parse(arguments).unwrap(), Request::Version);
        }
        for arguments in [
            ["run", "--help"].as_slice(),
            ["preview", "--help"].as_slice(),
            ["snapshot", "--help"].as_slice(),
            ["check", "-h"].as_slice(),
            ["new", "--help"].as_slice(),
            ["doctor", "-h"].as_slice(),
            ["mcp", "--help"].as_slice(),
            ["mcp", "-h"].as_slice(),
        ] {
            assert_eq!(parse(arguments).unwrap(), Request::Help);
        }
    }

    #[test]
    fn mcp_runs_one_tool_call() {
        assert_eq!(
            parse(&["mcp", "call", "click"]).unwrap(),
            Request::Execute(Command::McpCall {
                tool: "click".to_owned()
            })
        );
        assert!(parse(&["mcp"]).is_err());
        assert!(parse(&["mcp", "call"]).is_err());
        assert!(parse(&["init", "--claude"]).is_err());
    }

    #[test]
    fn typo_is_not_reinterpreted_using_the_filesystem() {
        assert_eq!(
            parse(&["rn"]).unwrap_err(),
            Error::UnknownCommand(OsString::from("rn"))
        );
    }

    #[test]
    fn missing_unknown_and_extra_arguments_are_precise() {
        assert_eq!(
            parse(&["new"]).unwrap_err(),
            Error::MissingArgument {
                command: "new",
                argument: "<path>"
            }
        );
        assert_eq!(
            parse(&["snapshot"]).unwrap_err(),
            Error::MissingArgument {
                command: "snapshot",
                argument: "--check or --update"
            }
        );
        assert_eq!(
            parse(&["--wat"]).unwrap_err(),
            Error::UnknownArgument(OsString::from("--wat"))
        );
        assert_eq!(
            parse(&["check", ".", "extra"]).unwrap_err(),
            Error::ExtraArgument(OsString::from("extra"))
        );
    }
}
