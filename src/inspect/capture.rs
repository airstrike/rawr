//! A capture of one of the program's windows, alone and without its
//! shadow, as a PNG: what `rawr mcp`'s screenshot and rawr's Take
//! Screenshot item both take (#1990), through this one function. It asks
//! the window server for the window by its number, so macOS asks for
//! Screen Recording permission, and a covered or locked window captures
//! stale or blank (#1989).

use std::fmt;
use std::path::Path;

/// Why a window wasn't captured.
#[derive(Debug)]
pub enum Error {
    /// The capture's file couldn't be written or read back.
    Io(std::io::Error),
    /// The capture tool failed.
    Failed(std::process::ExitStatus),
    /// This system can't capture a window.
    Unsupported,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Failed(status) => write!(formatter, "screencapture failed: {status}"),
            Self::Unsupported => formatter.write_str("a window capture needs macOS here"),
        }
    }
}

impl std::error::Error for Error {}

/// Capture the window the window server numbers `number` into `path`, and
/// return the PNG.
///
/// # Errors
///
/// When the capture fails or its file can't be written or read.
#[cfg(target_os = "macos")]
pub fn window(number: i64, path: &Path) -> Result<Vec<u8>, Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(Error::Io)?;
    }
    let status = std::process::Command::new("screencapture")
        .args(["-x", "-o", &format!("-l{number}")])
        .arg(path)
        .stdin(std::process::Stdio::null())
        .status()
        .map_err(Error::Io)?;
    if !status.success() {
        return Err(Error::Failed(status));
    }
    std::fs::read(path).map_err(Error::Io)
}

/// Capture the window the window server numbers `number` into `path`.
///
/// # Errors
///
/// Always: only macOS captures a window so far.
#[cfg(not(target_os = "macos"))]
pub fn window(_number: i64, _path: &Path) -> Result<Vec<u8>, Error> {
    Err(Error::Unsupported)
}
