// Copyright 2013-2016 The rust-url developers. MIT OR Apache-2.0.
// See base/PROVENANCE.md.

//! The registry caller's second parse, bound to its exact successful first input.

use super::super::{input::Input, output::Output, Failure, Invalid, Parse, Parser, Scheme, Url};
use crate::input::membership::manifest::{session::Session, Step};
use rawr_toml::document;
use std::path::Path;

/// The original text and first parse cannot be independently substituted.
pub(in crate::input::membership::manifest) struct Accepted<'a> {
    original: &'a str,
    first: Url,
}

/// Only native from_file_path can produce this base; it is never reparsed.
pub(in super::super) struct Native(Url);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::input::membership::manifest) enum BaseError {
    Path,
    Url(Invalid),
}

impl Url {
    pub(in crate::input::membership::manifest) fn accepted<'a, A: document::Admission>(
        original: &'a str,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Accepted<'a>, Invalid>> {
        Ok(Self::parse(original, session)?.map(|first| Accepted { original, first }))
    }
}

impl Accepted<'_> {
    pub(in crate::input::membership::manifest) fn url<A: document::Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<&Url> {
        session.check()?;
        Ok(&self.first)
    }

    #[cfg(unix)]
    pub(in crate::input::membership::manifest) fn with_file_base<A: document::Admission>(
        &self,
        path: &Path,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Url, BaseError>> {
        let base = match Url::from_file_path(path, session)? {
            Ok(url) => Native(url),
            Err(()) => return Ok(Err(BaseError::Path)),
        };
        // First success establishes an explicit scheme. The exact same input
        // is parsed again only after native base construction, including when
        // the second parser does not consult the base for a non-file scheme.
        let input = Input::new(self.original, session)?;
        let parser = Parser {
            output: Output::new(),
            session,
        };
        let result = match parser.absolute_in(input, Some(&base)) {
            Ok(url) => Ok(Ok(url)),
            Err(Failure::Invalid(error)) => Ok(Err(BaseError::Url(error))),
            Err(Failure::Stop(stop)) => Err(stop),
        };
        // The caller retains the first URL through its subsequent password check.
        result
    }
}

impl<A: document::Admission> Parser<'_, '_, A> {
    pub(in super::super) fn file_with_native(
        mut self,
        input: Input<'_>,
        base: &Native,
    ) -> Parse<Url> {
        let (first, after_first) = input.split_first();
        if matches!(first, Some('/' | '\\')) {
            let (second, _) = after_first.split_first();
            if matches!(second, Some('/' | '\\')) {
                return self.file(input);
            }
            self.output.append("file://", self.session)?;
            if !starts_drive(&after_first) {
                let first_segment = base.0.as_str(self.session)?[8..]
                    .split('/')
                    .next()
                    .unwrap_or("");
                if first_segment.len() == 2
                    && first_segment.as_bytes()[0].is_ascii_alphabetic()
                    && first_segment.as_bytes()[1] == b':'
                {
                    self.output.push('/', self.session)?;
                    self.output.append(first_segment, self.session)?;
                }
                // Unix from_file_path never has a host to inherit.
            }
            let remaining = self.path(Scheme::File, &mut false, 7, input)?;
            return self.finish(Scheme::File, 4, 7, remaining, false);
        }
        if matches!(first, None | Some('?' | '#')) {
            self.output
                .append(base.0.as_str(self.session)?, self.session)?;
            // Native from_file_path has no query or fragment; all three source
            // copy boundaries therefore coincide without removing path bytes.
            return self.finish(Scheme::File, 4, 7, input, false);
        }
        if starts_drive(&input) {
            return self.file(input);
        }
        self.output
            .append(base.0.as_str(self.session)?, self.session)?;
        self.shorten(Scheme::File, 7)?;
        let remaining = self.path(Scheme::File, &mut true, 7, input)?;
        self.finish(Scheme::File, 4, 7, remaining, false)
    }
}

fn starts_drive(input: &Input<'_>) -> bool {
    let mut input = input.clone();
    match (input.next(), input.next(), input.next()) {
        (Some(a), Some(':' | '|'), third) if a.is_ascii_alphabetic() => {
            matches!(third, None | Some('/' | '\\' | '?' | '#'))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests;
