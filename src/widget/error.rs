//! Structured failures while promoting or binding scripted widgets.

use crate::{application, host};
use std::fmt;

/// A symbolic scripted-widget proof did not match the current launch.
#[derive(Debug)]
pub enum Error {
    /// The current executable has no compatible promotion at the retained site.
    Site,
    /// The promotion's exact Message differs from the active selection.
    Message {
        /// Message retained by the symbolic descriptor.
        retained: String,
        /// Message required by the current application or preview.
        current: String,
    },
    /// The owned nominal widget data failed its current exact schema proof.
    Data(host::runtime::ValueError),
    /// A preview key does not identify a current compiler-validated preview.
    Preview,
}

impl Error {
    pub(crate) fn message(retained: &application::Type, current: &application::Type) -> Self {
        Self::Message {
            retained: retained.display_name(),
            current: current.display_name(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Site => formatter.write_str(
                "scripted widget site or protocol does not match the current executable",
            ),
            Self::Message { retained, current } => write!(
                formatter,
                "scripted widget Message `{retained}` does not match current Message `{current}`"
            ),
            Self::Data(error) => error.fmt(formatter),
            Self::Preview => formatter.write_str("scripted widget preview is not current"),
        }
    }
}

#[cfg(not(feature = "no_std"))]
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Data(error) => Some(error),
            Self::Site | Self::Message { .. } | Self::Preview => None,
        }
    }
}

#[cfg(feature = "no_std")]
impl core_error::Error for Error {
    fn source(&self) -> Option<&(dyn core_error::Error + 'static)> {
        match self {
            Self::Data(error) => Some(error),
            Self::Site | Self::Message { .. } | Self::Preview => None,
        }
    }
}
