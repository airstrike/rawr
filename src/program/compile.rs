//! Structured failures while compiling and authorizing a program.

use crate::{program, ParseError};
use std::error::Error as StdError;
use std::fmt;

/// A program could not be compiled into an authorized executable.
#[derive(Debug)]
pub enum Error {
    /// Source parsing, resolution, or lowering failed.
    Compilation(ParseError),
    /// The compiled executable could not be authorized by the host engine.
    Authorization(program::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Compilation(error) => error.fmt(formatter),
            Self::Authorization(error) => error.fmt(formatter),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Compilation(error) => Some(error),
            Self::Authorization(error) => Some(error),
        }
    }
}

impl From<ParseError> for Error {
    fn from(error: ParseError) -> Self {
        Self::Compilation(error)
    }
}

impl From<program::Error> for Error {
    fn from(error: program::Error) -> Self {
        Self::Authorization(error)
    }
}
