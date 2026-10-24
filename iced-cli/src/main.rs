#![forbid(unsafe_code)]

use std::process::ExitCode;

fn main() -> ExitCode {
    // `rawr` asks what rawr-iced serves before handing it anything.
    if std::env::args_os().nth(1).is_some_and(|argument| argument == rawr_cli::layer::FLAG) {
        print!("{}", rawr_iced_cli::layer::MANIFEST);
        return ExitCode::SUCCESS;
    }
    // A stopped CLI retires the runner window and builds it owns.
    #[cfg(feature = "native-runner")]
    if let Err(error) = retire_on_termination() {
        eprintln!("warning: child processes may outlive an interrupted rawr: {error}");
    }
    #[cfg(feature = "import-baseline")]
    let (arguments, evidence, profile) =
        match rawr_iced_cli::evidence::bootstrap(std::env::args_os().skip(1)) {
            Ok(rawr_iced_cli::evidence::Bootstrap::Command(arguments)) => arguments.into_parts(),
            Ok(rawr_iced_cli::evidence::Bootstrap::Cell(pending)) => {
                return match rawr_iced_cli::evidence::execute_cell(pending) {
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
    let request = rawr_iced_cli::command::parse(arguments);
    #[cfg(not(feature = "import-baseline"))]
    let request = rawr_iced_cli::command::parse(std::env::args_os().skip(1));
    #[cfg(feature = "import-baseline")]
    let profile = match request.as_ref() {
        Ok(request) => match rawr_iced_cli::evidence::authorize_request(profile, request) {
            Ok(profile) => profile,
            Err(error) => {
                eprintln!("error: {error}");
                return ExitCode::from(2);
            }
        },
        Err(_) => profile,
    };

    match request {
        Ok(rawr_iced_cli::command::Request::Help) => {
            println!("{}", rawr_iced_cli::command::HELP);
            ExitCode::SUCCESS
        }
        Ok(rawr_iced_cli::command::Request::Version) => {
            println!("rawr-iced {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Ok(rawr_iced_cli::command::Request::Execute(command)) => {
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
            let result = rawr_iced_cli::execute_with_profile(command, profile);
            #[cfg(not(feature = "import-baseline"))]
            let result = rawr_iced_cli::execute(command);
            #[cfg(feature = "import-baseline")]
            if let Some(active) = evidence {
                match &result {
                    Err(error) => {
                        active.finish_failure(&rawr_iced_cli::diagnostic::render(&error.report()))
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
                    eprint!("{}", rawr_iced_cli::diagnostic::render(&error.report()));
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

/// Make SIGINT, SIGTERM and SIGHUP retire every child process group this
/// process owns (a runner and its window, a Cargo build) before it exits with
/// the conventional status. Called before any thread starts: threads inherit
/// the blocked mask, while spawned programs start with an empty one.
#[cfg(feature = "native-runner")]
fn retire_on_termination() -> std::io::Result<()> {
    use nix::sys::signal::{SigSet, Signal};

    let mut signals = SigSet::empty();
    for signal in [Signal::SIGINT, Signal::SIGTERM, Signal::SIGHUP] {
        signals.add(signal);
    }
    signals.thread_block().map_err(std::io::Error::from)?;
    std::thread::Builder::new()
        .name("rawr-termination".into())
        .spawn(move || {
            let signal = loop {
                if let Ok(signal) = signals.wait() {
                    break signal;
                }
            };
            let _owned = rawr_import::process::retire_owned();
            std::process::exit(128 + signal as i32);
        })
        .map(drop)
}
