#![forbid(unsafe_code)]

use std::process::ExitCode;

fn main() -> ExitCode {
    // A stopped CLI retires the runner window and builds it owns.
    #[cfg(feature = "native-runner")]
    if let Err(error) = rawr_import::process::retire_on_termination() {
        eprintln!("warning: child processes may outlive an interrupted rawr: {error}");
    }
    #[cfg(feature = "import-baseline")]
    let (arguments, evidence, profile) =
        match rawr_cli::evidence::bootstrap(std::env::args_os().skip(1)) {
            Ok(rawr_cli::evidence::Bootstrap::Command(arguments)) => arguments.into_parts(),
            Ok(rawr_cli::evidence::Bootstrap::Cell(pending)) => {
                return match rawr_cli::evidence::execute_cell(pending) {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(error) => {
                        eprintln!("error: {error}");
                        ExitCode::FAILURE
                    }
                };
            }
            Err(error) => {
                eprintln!("error: {error}");
                return ExitCode::from(2);
            }
        };
    #[cfg(feature = "import-baseline")]
    let request = rawr_cli::command::parse(arguments);
    #[cfg(not(feature = "import-baseline"))]
    let request = rawr_cli::command::parse(std::env::args_os().skip(1));
    #[cfg(feature = "import-baseline")]
    let profile = match request.as_ref() {
        Ok(request) => match rawr_cli::evidence::authorize_request(profile, request) {
            Ok(profile) => profile,
            Err(error) => {
                eprintln!("error: {error}");
                return ExitCode::from(2);
            }
        },
        Err(_) => profile,
    };

    match request {
        Ok(rawr_cli::command::Request::Help) => {
            println!("{}", rawr_cli::command::HELP);
            ExitCode::SUCCESS
        }
        Ok(rawr_cli::command::Request::Version) => {
            println!("rawr {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Ok(rawr_cli::command::Request::Execute(command)) => {
            #[cfg(feature = "import-baseline")]
            let evidence = match evidence {
                Some(pending) => match pending.authenticate(&command) {
                    Ok(active) => Some(active),
                    Err(error) => {
                        eprintln!("error: {error}");
                        return ExitCode::from(2);
                    }
                },
                None => None,
            };
            #[cfg(feature = "import-baseline")]
            let result = rawr_cli::execute_with_profile(command, profile);
            #[cfg(not(feature = "import-baseline"))]
            let result = rawr_cli::execute(command);
            #[cfg(feature = "import-baseline")]
            if let Some(active) = evidence {
                match &result {
                    Err(error) => {
                        active.finish_failure(&rawr_cli::diagnostic::render(&error.report()))
                    }
                    Ok(Some(report)) if !report.is_successful() => {
                        active.finish_failure(&report.message())
                    }
                    _ => active.finish(rawr_import::metrics::evidence::capsule::Product::Success),
                }
            }
            match result {
                Ok(Some(report)) => {
                    if !report.message().is_empty() {
                        print!("{}", report.message());
                        if !report.message().ends_with('\n') {
                            println!();
                        }
                    }
                    if let Some(warning) = report.warning() {
                        eprintln!("{warning}");
                    }
                    if report.is_successful() {
                        ExitCode::SUCCESS
                    } else {
                        ExitCode::FAILURE
                    }
                }
                Ok(None) => ExitCode::SUCCESS,
                Err(error) => {
                    eprint!("{}", rawr_cli::diagnostic::render(&error.report()));
                    ExitCode::FAILURE
                }
            }
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(2)
        }
    }
}
