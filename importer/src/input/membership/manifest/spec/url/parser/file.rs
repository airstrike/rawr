// Copyright 2013-2016 The rust-url developers. MIT OR Apache-2.0.

//! All file states reached without a base URL, on every supported host platform.

pub(super) mod base;

use rawr_toml::document;

use super::{host, input::Input, path, Failure, Parse, Parser, Scheme, Url};

impl Url {
    /// Pinned url 2.5.8 Unix from_file_path; serialization is not reparsed.
    #[cfg(unix)]
    pub(in crate::input::membership::manifest) fn from_file_path<A: document::Admission>(
        path: &std::path::Path,
        session: &mut crate::input::membership::manifest::session::Session<'_, A>,
    ) -> crate::input::membership::manifest::Step<Result<Self, ()>> {
        use super::output::{Output, Set};
        use std::os::unix::ffi::OsStrExt;
        session.check()?;
        let mut output = Output::new();
        output.append("file://", session)?;
        session.string(path.as_os_str().as_bytes().len())?;
        if !path.is_absolute() {
            return Ok(Err(()));
        }
        let mut empty = true;
        for component in path.components().skip(1) {
            session.items(1)?;
            empty = false;
            output.push('/', session)?;
            output.encode_bytes(
                component.as_os_str().as_bytes(),
                Set::SpecialPathSegment,
                session,
            )?;
        }
        if empty {
            output.push('/', session)?;
        }
        Ok(Ok(Self {
            output,
            scheme_end: 4,
            path_start: 7,
            query_start: None,
            fragment_start: None,
        }))
    }
}

#[cfg(test)]
mod tests;

impl<A: document::Admission> Parser<'_, '_, A> {
    pub(super) fn file(mut self, input: Input<'_>) -> Parse<Url> {
        let (first, after_first) = input.split_first();
        if matches!(first, Some('/' | '\\')) {
            let (second, after_second) = after_first.split_first();
            self.output.append("file://", self.session)?;
            if matches!(second, Some('/' | '\\')) {
                let (text, remaining) = self.host_text(after_second.clone(), Scheme::File)?;
                let text = text.as_str(self.session)?;
                let (mut has_host, remaining) = if path::drive(text.as_bytes()) {
                    (false, after_second)
                } else if text.is_empty() {
                    (false, remaining)
                } else {
                    let host = host::Host::special(text, self.session)?
                        .map_err(|error| Failure::Invalid(error.into()))?;
                    let local = matches!(host.kind(), host::Kind::Domain(domain) if domain.as_bytes() == b"localhost");
                    if !local {
                        self.append_host(&host)?;
                    }
                    (!local, remaining)
                };
                let host_end = self.offset()?;
                let remaining = if has_host {
                    self.path_start(Scheme::File, &mut has_host, remaining)?
                } else {
                    let start = self.output.len();
                    self.output.push('/', self.session)?;
                    self.path(Scheme::File, &mut has_host, start, remaining)?
                };
                let path_start = if has_host {
                    host_end
                } else {
                    self.output.remove(7..host_end as usize, self.session)?;
                    7
                };
                return self.finish(Scheme::File, 4, path_start, remaining, false);
            }
            let remaining = self.path(Scheme::File, &mut false, 7, input)?;
            return self.finish(Scheme::File, 4, 7, remaining, false);
        }
        self.output.append("file:///", self.session)?;
        let remaining = self.path(Scheme::File, &mut false, 7, input)?;
        self.finish(Scheme::File, 4, 7, remaining, false)
    }
}
