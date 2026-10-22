// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.

//! Pinned build-script and readme normalization over recorded file predicates.

use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use crate::input::membership::host;

use super::super::super::{schema::primitive, session::Session, value, Step};
use super::super::path;

/// Unchanged typed text is borrowed; lexical build normalization owns its result.
#[derive(Debug)]
pub(in super::super) enum Text<'d> {
    Borrowed(value::Text<'d>),
    Owned(String),
}

impl Text<'_> {
    pub(in super::super) fn as_str(&self) -> &str {
        match self {
            Self::Borrowed(text) => text.as_str(),
            Self::Owned(text) => text,
        }
    }
}

/// Actual normalized variants. This helper never produces Auto(true) or absence.
#[derive(Debug)]
pub(in super::super) enum Build<'a, 'd> {
    Auto(bool),
    Single(Text<'d>),
    Multiple(&'a [value::Text<'d>]),
}

/// The caller performs embedded handling and the multiple-build feature gate
/// before invoking this source helper.
pub(in super::super) fn build<'a, 'd>(
    input: Option<&'a primitive::Build<'d>>,
    root: &Path,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<Build<'a, 'd>> {
    session.check()?;
    let result = match input {
        None => {
            let candidate = path::join(root, Path::new("build.rs"), session)?;
            if session.is_file(candidate.as_os_str().as_bytes())? {
                Build::Single(borrowed(value::Text::Borrowed("build.rs"), session)?)
            } else {
                Build::Auto(false)
            }
        }
        Some(primitive::Build::Auto(false)) => Build::Auto(false),
        Some(primitive::Build::Auto(true)) => {
            Build::Single(borrowed(value::Text::Borrowed("build.rs"), session)?)
        }
        Some(primitive::Build::Single(text)) => {
            Build::Single(Text::Owned(path::normalize_text(text.as_str(), session)?))
        }
        Some(primitive::Build::Multiple(texts)) => Build::Multiple(texts.as_slice()),
    };
    session.items(1)?;
    Ok(result)
}

/// Explicit readme values do not probe or normalize their text. Absent readme
/// tries the pinned defaults in order and stops at the first recorded file.
pub(in super::super) fn readme<'d>(
    root: &Path,
    input: Option<&primitive::StringOrBool<'d>>,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<Option<Text<'d>>> {
    session.check()?;
    let selected = match input {
        Some(primitive::StringOrBool::Boolean(false)) => None,
        Some(primitive::StringOrBool::Boolean(true)) => Some(value::Text::Borrowed("README.md")),
        Some(primitive::StringOrBool::String(text)) => Some(*text),
        None => {
            let mut selected = None;
            for name in ["README.md", "README.txt", "README"] {
                let candidate = path::join(root, Path::new(name), session)?;
                if session.is_file(candidate.as_os_str().as_bytes())? {
                    selected = Some(value::Text::Borrowed(name));
                    break;
                }
            }
            selected
        }
    };
    let result = selected.map(|text| borrowed(text, session)).transpose()?;
    session.items(1)?;
    Ok(result)
}

fn borrowed<'d>(
    text: value::Text<'d>,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<Text<'d>> {
    session.string(text.as_str().len())?;
    Ok(Text::Borrowed(text))
}

#[cfg(test)]
mod tests;
