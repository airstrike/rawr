// Copyright the ripgrep contributors. MIT OR Unlicense.
//! The required-extension and regex-set slices of globset 0.4.18 lib.rs.
//! These are real strategies, not a partial GlobSet dispatch or fallback.
use super::{regex, table};
use crate::input::membership::manifest::{
    load::formatted, session::Session, storage::Buffer, Error, Step,
};
use rawr_toml::document::Admission;

mod compile;
pub(super) mod edge;
pub(super) mod multi;
pub(super) mod required;
pub(super) mod set;

#[cfg(test)]
mod tests;

/// The upstream single-regex error owns its pattern; regex-set errors have no
/// pattern context. ErrorKind::Regex owns the eagerly rendered diagnostic.
pub(in super::super) struct Failure {
    pub(super) pattern: Option<String>,
    pub(super) message: String,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.pattern {
            None => formatter.write_str(&self.message),
            Some(pattern) => write!(
                formatter,
                "error parsing glob '{pattern}': {}",
                self.message
            ),
        }
    }
}
