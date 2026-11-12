//! Window frames carried across the restarts of a program `rawr run` runs
//! (D952, #1811): a restart, a respawn (D943) or the next day's cold start,
//! shows each window where the person left it, unless the program itself
//! now asks for another frame.
//!
//! The rule is the platform's least intrusive one. Each run records, for
//! each window, the frame it first showed with and the frame it was last
//! in. A window of the next run that first shows with the same frame as the
//! one it matches gets that one's last frame; a window the program shows
//! elsewhere or at another size shows as cargo shows it. A window is
//! matched by the stable identifier the program gave it, or else by its
//! order among the run's windows while the count open matches. Panels,
//! sheets, popovers and other transient windows aren't followed.
//!
//! `record` and `session` hold that rule and know no platform. macOS
//! follows windows through AppKit's notifications; another platform adds a
//! module beside `appkit` that hands its windows to a `Session`, and
//! Wayland, where a client can't place its window, adds none.

#[cfg(target_os = "macos")]
mod appkit;
// The rule waits for its platform's module beside `appkit`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod record;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod session;

use std::path::Path;

/// Follow the windows this process shows and keep their frames in a file
/// under `directory`, the project's `target/rawr`, written at each show,
/// move and resize and read here. Call it on the main thread, before the
/// program's `main` runs. A process that loaded no windowing library, and
/// any platform but macOS, keeps nothing.
pub fn keep(directory: &Path) {
    let program = std::env::args_os().next();
    let name = program
        .as_deref()
        .map(Path::new)
        .and_then(Path::file_name)
        .unwrap_or("program".as_ref());
    let file = directory.join("windows").join(name);
    #[cfg(target_os = "macos")]
    appkit::keep(file);
    #[cfg(not(target_os = "macos"))]
    let _ = file;
}
