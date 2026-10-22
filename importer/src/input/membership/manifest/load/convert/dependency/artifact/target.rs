//! CompileTarget's pure stages around the unqualified native operation.
use crate::input::membership::manifest::{
    load::normalize::target::formatted, session::Session, Error, Source, Step,
};
use rawr_toml::document;
use std::{ffi::OsStr, os::unix::ffi::OsStrExt, path::Path};

pub(in super::super::super::super) enum Target {
    Assume,
    Tuple(String),
    Json { path: String, short: String },
}
pub(in super::super::super::super) enum Progress<'a> {
    Ready(Target),
    Canonical(Request<'a>),
}
pub(in super::super::super::super) struct Request<'a> {
    spelling: &'a str,
    _input: Option<Vec<u8>>,
}
impl Request<'_> {
    pub(in super::super::super::super) fn spelling(&self) -> &str {
        self.spelling
    }

    // Deliberately private until an associated native observation can discharge
    // this request. Tests qualify only the pure post-native transformation.
    fn canonical<A: document::Admission>(
        self,
        bytes: &[u8],
        session: &mut Session<'_, A>,
    ) -> Step<Result<Target, String>> {
        session.check()?;
        session.string(bytes.len())?;
        let text = match std::str::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) => {
                return formatted(
                    format_args!("target path `{}` is not valid unicode", self.spelling),
                    session,
                )
                .map(Err)
            }
        };
        // Pinned source owns/interns full Unicode path before extracting stem.
        let path = formatted(format_args!("{text}"), session)?;
        session.items(1)?;
        let Some(stem) = Path::new(OsStr::from_bytes(bytes)).file_stem() else {
            session.items(1)?;
            return session.fail(Error::Source {
                cause: Source::CompileTargetStem,
                span: None,
            });
        };
        let Some(stem) = stem.to_str() else {
            return session.fail(Error::State);
        };
        let short = formatted(format_args!("{stem}"), session)?;
        Ok(Ok(Target::Json { path, short }))
    }
}

pub(super) fn parse<'a, A: document::Admission>(
    name: &'a str,
    json: bool,
    session: &mut Session<'_, A>,
) -> Step<Result<Progress<'a>, String>> {
    session.check()?;
    session.string(name.len())?;
    // ArtifactTarget recognizes the literal before CompileTarget trims.
    if name == "target" {
        return Ok(Ok(Progress::Ready(Target::Assume)));
    }
    admitted(name, json, session)
}

pub(in super::super::super::super) fn compile<'a, A: document::Admission>(
    name: &'a str,
    json: bool,
    session: &mut Session<'_, A>,
) -> Step<Result<Progress<'a>, String>> {
    session.check()?;
    session.string(name.len())?;
    admitted(name, json, session)
}

fn admitted<'a, A: document::Admission>(
    name: &'a str,
    json: bool,
    session: &mut Session<'_, A>,
) -> Step<Result<Progress<'a>, String>> {
    let name = name.trim();
    if name.is_empty() {
        return formatted(format_args!("target was empty"), session).map(Err);
    }
    if !name.ends_with(".json") {
        return formatted(format_args!("{name}"), session)
            .map(|name| Ok(Progress::Ready(Target::Tuple(name))));
    }
    if !json {
        return formatted(format_args!("`.json` target specs require -Zjson-target-spec to be added to the cargo invocation"),session).map(Err);
    }
    let input = if name.len() >= 384 {
        let Some(length) = name.len().checked_add(1) else {
            return session.fail(Error::Overflow);
        };
        session.admit(document::Request::Allocation {
            bytes: length,
            alignment: 1,
        })?;
        let mut bytes = Vec::new();
        if let Err(error) = bytes.try_reserve_exact(length) {
            return session.fail(Error::Allocation(error));
        }
        bytes.extend_from_slice(name.as_bytes());
        bytes.push(0);
        Some(bytes)
    } else {
        None
    };
    // No errno is fabricated for std's source-local InvalidInput.
    if name.as_bytes().contains(&0) {
        // CString's failure payload is dropped before std returns NUL_ERR.
        // Preserve that lifetime boundary; the original account is not refunded.
        drop(input);
        return formatted(format_args!("target path `{name}` is not a valid file: file name contained an unexpected NUL byte"),session).map(Err);
    }
    Ok(Ok(Progress::Canonical(Request {
        spelling: name,
        _input: input,
    })))
}

#[cfg(test)]
mod tests;
