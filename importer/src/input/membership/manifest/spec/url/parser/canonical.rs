// Copyright the Cargo contributors and the rust-url developers. MIT OR Apache-2.0.
// See canonical/PROVENANCE.md.

//! Cargo comparison URL, transformed separately from the original fetch URL.

use rawr_toml::document;

use super::{input::Input, output::Output, path::Context, Failure, Parser, Scheme, Url};
use crate::input::membership::manifest::{load::formatted, session::Session, Error, Step};

/// Only the completed Cargo canonicalization grants comparison identity.
#[derive(Eq, PartialEq, Ord, PartialOrd)]
pub(in crate::input::membership::manifest) struct Canonical(Url);

impl Canonical {
    pub(in crate::input::membership::manifest) fn from<A: document::Admission>(
        original: &Url,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Self, String>> {
        let mut output = Output::new();
        output.append(original.as_str(session)?, session)?;
        session.items(1)?;
        let mut url = Url {
            output,
            scheme_end: original.scheme_end,
            path_start: original.path_start,
            query_start: original.query_start,
            fragment_start: original.fragment_start,
        };
        if !url.as_str(session)?[url.scheme_end as usize + 1..].starts_with('/') {
            return Ok(Err(formatted(
                format_args!(
                    "invalid url `{}`: cannot-be-a-base-URLs are not supported",
                    url.as_str(session)?
                ),
                session,
            )?));
        }
        if path(&url, session)?.ends_with('/') {
            url = mutate(url, Mutation::PopEmpty, session)?;
        }
        if !url.scheme(session)?.contains('+') {
            if github(&url, session)? {
                let mut rewritten = Output::new();
                rewritten.append("https", session)?;
                rewritten.append(&url.as_str(session)?[url.scheme_end as usize..], session)?;
                // The source actually reparses here: special-scheme rules change
                // ports, backslashes and query encoding, not merely the scheme.
                url = match Url::parse(rewritten.as_str(session)?, session)? {
                    Ok(url) => url,
                    Err(_) => return session.fail(Error::State),
                };
                drop(rewritten);
                let mut lower = Output::new();
                let current = path(&url, session)?;
                session.string(current.len())?;
                // HTTPS path serialization is ASCII: non-ASCII source scalars
                // have already been UTF-8 percent encoded. These mappings equal
                // str::to_lowercase (no contextual Unicode scalar is present).
                for character in current.chars().flat_map(char::to_lowercase) {
                    lower.push(character, session)?;
                }
                url = mutate(url, Mutation::Set(lower.as_str(session)?), session)?;
            }
            if path(&url, session)?.ends_with(".git") {
                let current = path(&url, session)?;
                let last = current.rsplit('/').next().ok_or(Error::State);
                let last = match last {
                    Ok(last) => last,
                    Err(error) => return session.fail(error),
                };
                let mut segment = Output::new();
                segment.append(&last[..last.len() - 4], session)?;
                url = mutate(
                    url,
                    Mutation::ReplaceLast(segment.as_str(session)?),
                    session,
                )?;
            }
        }
        session.items(1)?;
        Ok(Ok(Self(url)))
    }

    pub(in crate::input::membership::manifest) fn as_str<A: document::Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<&str> {
        self.0.as_str(session)
    }
}

fn end(url: &Url) -> usize {
    url.query_start
        .or(url.fragment_start)
        .map_or(url.output.len(), |i| i as usize)
}

fn path<'a, A: document::Admission>(url: &'a Url, session: &mut Session<'_, A>) -> Step<&'a str> {
    Ok(&url.as_str(session)?[url.path_start as usize..end(url)])
}

fn github<A: document::Admission>(url: &Url, session: &mut Session<'_, A>) -> Step<bool> {
    let text = url.as_str(session)?;
    let authority = &text[url.scheme_end as usize + 1..url.path_start as usize];
    let Some(authority) = authority.strip_prefix("//") else {
        return Ok(false);
    };
    // Accepted serialization percent-encodes userinfo @; an IPv6 literal can
    // never equal this domain. No reparsing or allocation is needed to borrow it.
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    Ok(host_port.split(':').next() == Some("github.com"))
}

enum Mutation<'a> {
    PopEmpty,
    Set(&'a str),
    ReplaceLast(&'a str),
}

fn mutate<A: document::Admission>(
    mut url: Url,
    mutation: Mutation<'_>,
    session: &mut Session<'_, A>,
) -> Step<Url> {
    let old_end = end(&url);
    let mut tail = Output::new();
    tail.append(&url.as_str(session)?[old_end..], session)?;
    url.output.truncate(old_end, session)?;
    let scheme = Scheme::of(url.scheme(session)?);
    let path_start = url.path_start as usize;
    match mutation {
        Mutation::PopEmpty => {
            if path_start + 1 < url.output.len() && url.output.as_bytes().ends_with(b"/") {
                url.output.pop(session)?;
            }
        }
        Mutation::Set(value) => {
            url.output.truncate(path_start, session)?;
            let input = Input::no_trim(value, session)?;
            let mut parser = Parser {
                output: url.output,
                session,
            };
            match parser.path_start_in(scheme, &mut true, input, Context::Setter) {
                Ok(_) => {}
                Err(Failure::Stop(stop)) => return Err(stop),
                Err(Failure::Invalid(_)) => return parser.session.fail(Error::State),
            }
            url.output = parser.output;
        }
        Mutation::ReplaceLast(value) => {
            let first = path_start + 1;
            if first < url.output.len() {
                let last_slash = url.output.as_bytes()[first..]
                    .iter()
                    .rposition(|&b| b == b'/')
                    .unwrap_or(0);
                url.output.truncate(first + last_slash, session)?;
            }
            if !matches!(value, "." | "..") {
                if url.output.len() > path_start + 1 || url.output.len() == path_start {
                    url.output.push('/', session)?;
                }
                let input = Input::no_trim(value, session)?;
                let mut parser = Parser {
                    output: url.output,
                    session,
                };
                match parser.path_in(scheme, &mut true, path_start, input, Context::Segment) {
                    Ok(_) => {}
                    Err(Failure::Stop(stop)) => return Err(stop),
                    Err(Failure::Invalid(_)) => return parser.session.fail(Error::State),
                }
                url.output = parser.output;
            }
        }
    }
    let new_end = match url.output.offset(session)? {
        Ok(offset) => offset,
        Err(_) => return session.fail(Error::State),
    };
    for index in [&mut url.query_start, &mut url.fragment_start]
        .into_iter()
        .flatten()
    {
        let Some(adjusted) = (*index as usize)
            .checked_sub(old_end)
            .and_then(|delta| (new_end as usize).checked_add(delta))
            .and_then(|value| u32::try_from(value).ok())
        else {
            return session.fail(Error::State);
        };
        *index = adjusted;
    }
    url.output.append(tail.as_str(session)?, session)?;
    Ok(url)
}

#[cfg(test)]
mod tests;
