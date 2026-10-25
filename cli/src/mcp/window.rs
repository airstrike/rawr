//! A window-only capture of a program's window, found by the pids of the
//! process tree the server started (macOS). Any host's window counts, a
//! winit one included. macOS asks for Screen Recording permission for this
//! capture; an extension's own capture (iced's socket) needs none.

use std::path::Path;

/// One window: its number, its title and its owner's pid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Window {
    /// The window server's number for it.
    pub number: i64,
    /// Its title, when macOS shows it.
    pub title: String,
    /// The pid owning it.
    pub pid: u32,
}

/// The on-screen windows owned by `pids`.
#[cfg(target_os = "macos")]
#[must_use]
pub fn windows(pids: &[u32]) -> Vec<Window> {
    use core_foundation::base::CFType;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;
    use core_graphics::window::{
        create_description_from_array, create_window_list, kCGNullWindowID,
        kCGWindowListExcludeDesktopElements, kCGWindowListOptionOnScreenOnly,
    };

    let Some(identifiers) = create_window_list(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        kCGNullWindowID,
    ) else {
        return Vec::new();
    };
    let Some(descriptions) = create_description_from_array(identifiers) else {
        return Vec::new();
    };
    let field = |window: &CFDictionary<CFString, CFType>, key: &str| {
        window
            .find(CFString::new(key))
            .map(|value| (*value).clone())
    };
    let mut found = Vec::new();
    for window in descriptions.iter() {
        let number = |key| {
            field(&window, key)
                .and_then(|value| value.downcast::<CFNumber>())
                .and_then(|value| value.to_i64())
        };
        let Some(pid) = number("kCGWindowOwnerPID").and_then(|pid| u32::try_from(pid).ok()) else {
            continue;
        };
        if number("kCGWindowLayer") != Some(0) || !pids.contains(&pid) {
            continue;
        }
        let Some(window_number) = number("kCGWindowNumber") else {
            continue;
        };
        let title = field(&window, "kCGWindowName")
            .and_then(|value| value.downcast::<CFString>())
            .map(|value| value.to_string())
            .unwrap_or_default();
        found.push(Window {
            number: window_number,
            title,
            pid,
        });
    }
    found
}

#[cfg(not(target_os = "macos"))]
#[must_use]
pub fn windows(_pids: &[u32]) -> Vec<Window> {
    Vec::new()
}

/// Capture `window` alone, without its shadow, into `path`, and return the
/// PNG.
#[cfg(target_os = "macos")]
pub fn capture(window: &Window, path: &Path) -> Result<Vec<u8>, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let status = std::process::Command::new("screencapture")
        .args(["-x", "-o", &format!("-l{}", window.number)])
        .arg(path)
        .stdin(std::process::Stdio::null())
        .status()
        .map_err(|error| error.to_string())?;
    if !status.success() {
        return Err(format!("screencapture failed: {status}"));
    }
    std::fs::read(path).map_err(|error| error.to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn capture(_window: &Window, _path: &Path) -> Result<Vec<u8>, String> {
    Err("a window capture needs macOS here".to_owned())
}
