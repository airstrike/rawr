// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.
//! Cargo's string and native-path IntoUrl adapters; no source-ID authority.

use rawr_toml::document;
use std::path::Path;

use super::super::{
    super::{
        session::Session,
        spec::url::{Accepted, Invalid, Url},
        Step,
    },
    normalize::target,
};

pub(in super::super) fn parse<A: document::Admission>(
    text: &str,
    session: &mut Session<'_, A>,
) -> Step<Result<Url, String>> {
    let error = match Url::parse(text, session)? {
        Ok(url) => return Ok(Ok(url)),
        Err(error) => error,
    };
    invalid(text, error, session).map(Err)
}

pub(in super::super) fn accepted<'a, A: document::Admission>(
    text: &'a str,
    session: &mut Session<'_, A>,
) -> Step<Result<Accepted<'a>, String>> {
    match Url::accepted(text, session)? {
        Ok(value) => Ok(Ok(value)),
        Err(error) => invalid(text, error, session).map(Err),
    }
}

fn invalid<A: document::Admission>(
    text: &str,
    error: Invalid,
    session: &mut Session<'_, A>,
) -> Step<String> {
    session.items(1)?;
    if text.starts_with("git@") {
        // Cargo's replacen(':', "/", 1) owns a string even without a colon.
        session.string(text.len())?;
        session.items(1)?;
        let suggestion = match text.split_once(':') {
            Some((before, after)) => target::formatted(format_args!("{before}/{after}"), session)?,
            None => target::formatted(format_args!("{text}"), session)?,
        };
        target::formatted(
            format_args!("invalid url `{text}`: {error}; try using `ssh://{suggestion}` instead"),
            session,
        )
    } else {
        target::formatted(format_args!("invalid url `{text}`: {error}"), session)
    }
}

pub(in super::super) fn path<A: document::Admission>(
    path: &Path,
    session: &mut Session<'_, A>,
) -> Step<Result<Url, String>> {
    match Url::from_file_path(path, session)? {
        Ok(url) => Ok(Ok(url)),
        Err(()) => target::formatted(
            format_args!("invalid path url `{}`", path.display()),
            session,
        )
        .map(Err),
    }
}

#[cfg(test)]
mod tests;
