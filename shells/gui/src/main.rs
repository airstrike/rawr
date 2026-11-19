//! `rawr-gui <session socket>`: the window `rawr gui` opens on a run's
//! session.

use std::path::PathBuf;
use std::process::ExitCode;

use rawr_gui::App;

fn main() -> ExitCode {
    let mut arguments = std::env::args_os().skip(1);
    let (Some(socket), None) = (arguments.next(), arguments.next()) else {
        eprintln!("usage: rawr-gui <session socket>");
        return ExitCode::from(2);
    };
    // `rawr` asks each `rawr-*` binary beside it for an extension's
    // manifest. A clean exit without one says this is none.
    if socket == "--rawr-manifest" {
        return ExitCode::SUCCESS;
    }
    if socket.to_string_lossy().starts_with('-') {
        eprintln!("usage: rawr-gui <session socket>");
        return ExitCode::from(2);
    }
    let socket = PathBuf::from(socket);
    let ran = iced::application(move || App::boot(socket.clone()), App::update, App::view)
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .window_size((1200.0, 820.0))
        .run();
    match ran {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rawr gui: {error}");
            ExitCode::FAILURE
        }
    }
}
