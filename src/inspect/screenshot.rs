//! A capture of a running program's window (#1989), asked for on the
//! program's socket as `{"op": "screenshot", "title": .., "file": ..}`.
//!
//! The program captures its own window, in its own process: it draws a
//! frame of the window's content as it draws every frame, through its own
//! renderer, and reads that frame back. Nothing asks the window server for
//! the window's pixels, so the capture needs no permission, and it shows
//! the window as it is when another window covers it, when it is off
//! screen, or while the session is locked. A capture draws first, so a
//! window that stopped drawing while it was covered shows its current
//! state.
//!
//! The capture is the whole window: its title bar, drawn by the window's
//! own frame view, above the content, and the agent cursor where the last
//! input put it. Its size is the window's in logical points, title bar
//! included, the space the input tools take positions in, at the window's
//! backing scale.
//!
//! The answer comes once the PNG is written to `file`: `{"title": ..,
//! "width": .., "height": .., "scale": .., "windows": [..]}`, `windows`
//! being every window's title, front first; or `{"error": ..}`.

use std::fmt;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

/// A written capture: the window it shows and the program's windows.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Shot {
    /// The window's title.
    pub(super) title: String,
    /// The window's size in logical points, title bar included.
    pub(super) width: f64,
    pub(super) height: f64,
    /// Pixels per point.
    pub(super) scale: f64,
    /// Every window's title, front first.
    pub(super) windows: Vec<String>,
}

/// Why a capture wasn't written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Error {
    /// The request names no file to write.
    File,
    /// The program shows no window, or none with that title, after it
    /// showed its first: `shown` are the titles it shows.
    Window {
        title: Option<String>,
        shown: Vec<String>,
    },
    /// The window's content isn't drawn into a layer this capture reads.
    Layer,
    /// The window drew no frame when asked to.
    Undrawn,
    /// The frame's pixels are in a format this capture doesn't read.
    Format(u64),
    /// The PNG couldn't be made or written.
    Write(String),
    /// This system can't capture a window.
    #[cfg(not(target_os = "macos"))]
    Unsupported,
    /// The wait for the window was given up: the program paused, or the
    /// tool that asked went away.
    Interrupted,
    /// The window's event loop ended before it drew.
    Ended,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File => formatter.write_str("a screenshot needs a `file` to write"),
            Self::Window { title, shown } => {
                match title {
                    Some(title) => {
                        write!(formatter, "the program shows no window titled `{title}`")?
                    }
                    None => formatter.write_str("the program shows no window")?,
                }
                if !shown.is_empty() {
                    let titles: Vec<String> =
                        shown.iter().map(|title| format!("`{title}`")).collect();
                    write!(formatter, "; it shows {}", titles.join(", "))?;
                }
                Ok(())
            }
            Self::Layer => {
                formatter.write_str("the window's content isn't drawn into a Metal layer")
            }
            Self::Undrawn => formatter.write_str("the window drew no frame when asked to"),
            Self::Format(format) => {
                write!(
                    formatter,
                    "the window's frame has pixel format {format}, which a capture doesn't read"
                )
            }
            Self::Write(reason) => write!(formatter, "the capture couldn't be written: {reason}"),
            #[cfg(not(target_os = "macos"))]
            Self::Unsupported => formatter.write_str("capturing a window needs macOS here"),
            Self::Interrupted => {
                formatter.write_str("the program paused before its window drew the capture")
            }
            Self::Ended => formatter.write_str("the program's window ended before it drew"),
        }
    }
}

impl Shot {
    fn to_json(&self) -> Value {
        json!({
            "title": self.title,
            "width": self.width,
            "height": self.height,
            "scale": self.scale,
            "windows": self.windows,
        })
    }
}

/// Capture the window titled `title`, or the front one, into `file`, for
/// the socket or for rawr's Take Screenshot item (#1990).
pub(super) fn capture(
    title: Option<&str>,
    file: &Path,
    interrupted: &dyn Fn() -> bool,
) -> Result<Shot, Error> {
    #[cfg(target_os = "macos")]
    {
        super::input::capture(title, file, interrupted)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (title, file, interrupted);
        Err(Error::Unsupported)
    }
}

/// Answer one capture request, or `None` once `interrupted` gave it up.
pub(super) fn answer(request: &Value, interrupted: &dyn Fn() -> bool) -> Option<Value> {
    let title = request.get("title").and_then(Value::as_str);
    let file = request
        .get("file")
        .and_then(Value::as_str)
        .map(PathBuf::from);
    let shot = file
        .ok_or(Error::File)
        .and_then(|file| capture(title, &file, interrupted));
    match shot {
        Ok(shot) => Some(shot.to_json()),
        Err(Error::Interrupted) => None,
        Err(error) => Some(json!({ "error": error.to_string() })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_window_names_the_ones_shown() {
        let error = Error::Window {
            title: Some("Settings".to_owned()),
            shown: vec!["Counter".to_owned(), "About".to_owned()],
        };
        assert_eq!(
            error.to_string(),
            "the program shows no window titled `Settings`; it shows `Counter`, `About`"
        );
        assert_eq!(
            answer(&json!({ "title": "x" }), &|| false).unwrap()["error"],
            "a screenshot needs a `file` to write"
        );
    }
}
