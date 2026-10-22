//! Complete pinned package-spec scalar transformation for manifest map keys.
// Copyright The Rust Project Developers. MIT OR Apache-2.0.
// See spec/PROVENANCE.md and the retained reference Cargo licenses.

mod display;
mod query;
#[cfg(test)]
pub(in crate::input::membership::manifest) mod tests;
pub(super) mod url;
mod version;
pub(super) use version::Full;

use std::alloc::Layout;

use rawr_toml::document;

use super::{scalar, session, Diagnostic, Error, Span, Step, Tag};

/// A complete interpreted package key; no source membership authority.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Spec {
    name: String,
    version: Option<version::Version>,
    url: Option<url::Url>,
    kind: Option<Kind>,
}

// Cargo SourceKind's manual order, not its declaration order. LocalRegistry
// and Directory cannot be produced by the pinned PackageIdSpec parser.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Kind {
    Path,
    Registry,
    SparseRegistry,
    Git(query::Reference<'static>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Invalid {
    Query,
    PathScheme,
    Protocol,
    MissingPath,
    EmptyFragment,
}

impl Spec {
    /// Cargo's replacement caller fills only an absent URL; source kind stays intact.
    pub(super) fn default_registry<A: document::Admission>(
        &mut self,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        session.check()?;
        if self.url.is_none() {
            let url =
                match url::Url::parse("https://github.com/rust-lang/crates.io-index", session)? {
                    Ok(url) => url,
                    Err(_) => return session.fail(Error::State),
                };
            self.url = Some(url);
        }
        Ok(())
    }
    pub(super) fn name(&self) -> &str {
        &self.name
    }
    pub(super) fn version(&self) -> Option<&version::Version> {
        self.version.as_ref()
    }
    pub(super) fn full_version<A: document::Admission>(
        &self,
        session: &mut session::Session<'_, A>,
    ) -> Step<Option<Full>> {
        session.check()?;
        match &self.version {
            Some(version) => version.full(session),
            None => Ok(None),
        }
    }
    pub(super) fn kind(&self) -> Option<&Kind> {
        self.kind.as_ref()
    }

    pub(super) fn parse<A: document::Admission>(
        text: &str,
        span: Option<Span>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        session.string(text.len())?;
        if text.contains("://")
            && let Ok(url) = url::Url::parse(text, session)?
        {
            return Self::from_url(url, span, session);
        }
        // Only the rejecting non-URL path diagnostic probes are omitted.
        // The complete plain grammar still runs, including version-before-name.
        let (name, version) = match split(text, span, session)? {
            Some(parts) => parts,
            None => (owned(text, session)?, None),
        };
        valid_name(&name, span, session)?;
        session.items(1)?;
        Ok(Self {
            name,
            version,
            url: None,
            kind: None,
        })
    }

    fn from_url<A: document::Admission>(
        mut url: url::Url,
        span: Option<Span>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        let kind = if let Some((protocol, scheme)) = url.scheme(session)?.split_once('+') {
            match protocol {
                "git" => {
                    let reference = query::parse(url.query(session)?.unwrap_or(""), session)?
                        .into_owned(session)?;
                    url.remove_query(session)?;
                    url = reparse(&url, span, session)?;
                    Some(Kind::Git(reference))
                }
                "registry" | "sparse" | "path" => {
                    if url.query(session)?.is_some() {
                        return reject(Invalid::Query, span, session);
                    }
                    if protocol == "path" && scheme != "file" {
                        return reject(Invalid::PathScheme, span, session);
                    }
                    let kind = match protocol {
                        "path" => Kind::Path,
                        "registry" => Kind::Registry,
                        _ => Kind::SparseRegistry,
                    };
                    if !matches!(kind, Kind::SparseRegistry) {
                        url = reparse(&url, span, session)?;
                    }
                    Some(kind)
                }
                _ => return reject(Invalid::Protocol, span, session),
            }
        } else {
            if url.query(session)?.is_some() {
                return reject(Invalid::Query, span, session);
            }
            None
        };
        let fragment = url
            .fragment(session)?
            .map(|text| owned(text, session))
            .transpose()?;
        url.remove_fragment(session)?;
        let Some(path_name) = url.last_path_segment(session)? else {
            return reject(Invalid::MissingPath, span, session);
        };
        let (name, version) = match fragment {
            Some(fragment) => match split(&fragment, span, session)? {
                Some(parts) => parts,
                None => {
                    let Some(first) = fragment.chars().next() else {
                        return reject(Invalid::EmptyFragment, span, session);
                    };
                    if first.is_alphabetic() {
                        (fragment, None)
                    } else {
                        let version = parsed_version(&fragment, span, session)?;
                        (owned(path_name, session)?, Some(version))
                    }
                }
            },
            None => (owned(path_name, session)?, None),
        };
        valid_name(&name, span, session)?;
        session.items(1)?;
        Ok(Self {
            name,
            version,
            url: Some(url),
            kind,
        })
    }
}

fn split<A: document::Admission>(
    text: &str,
    span: Option<Span>,
    session: &mut session::Session<'_, A>,
) -> Step<Option<(String, Option<version::Version>)>> {
    session.check()?;
    let Some((name, version)) = text.rsplit_once('@').or_else(|| {
        text.rsplit_once(':')
            .filter(|(name, _)| !name.ends_with(':'))
    }) else {
        return Ok(None);
    };
    let name = owned(name, session)?;
    let version = parsed_version(version, span, session)?;
    Ok(Some((name, Some(version))))
}

fn parsed_version<A: document::Admission>(
    text: &str,
    span: Option<Span>,
    session: &mut session::Session<'_, A>,
) -> Step<version::Version> {
    session.items(1)?;
    version::Version::parse(text, span, session)
}

fn reparse<A: document::Admission>(
    url: &url::Url,
    span: Option<Span>,
    session: &mut session::Session<'_, A>,
) -> Step<url::Url> {
    let Some((_, suffix)) = url.as_str(session)?.split_once('+') else {
        return session.fail(Error::State);
    };
    match url::Url::parse(suffix, session)? {
        Ok(url) => Ok(url),
        Err(_) => session.source_reparse(span),
    }
}

fn valid_name<A: document::Admission>(
    name: &str,
    span: Option<Span>,
    session: &mut session::Session<'_, A>,
) -> Step<()> {
    session.check()?;
    match scalar::Name::parse(name, scalar::Kind::Package) {
        Ok(_) => Ok(()),
        Err(error) => session.reject(Diagnostic::at(Tag::Scalar(error), span)),
    }
}

fn reject<T, A: document::Admission>(
    reason: Invalid,
    span: Option<Span>,
    session: &mut session::Session<'_, A>,
) -> Step<T> {
    session.reject(Diagnostic::at(Tag::Spec(reason), span))
}

fn owned<A: document::Admission>(
    text: &str,
    session: &mut session::Session<'_, A>,
) -> Step<String> {
    let Ok(layout) = Layout::array::<u8>(text.len()) else {
        return session.fail(Error::Overflow);
    };
    session.string(text.len())?;
    session.items(1)?;
    session.admit(document::Request::Allocation {
        bytes: layout.size(),
        alignment: layout.align(),
    })?;
    let mut output = String::new();
    if let Err(error) = output.try_reserve_exact(text.len()) {
        return session.fail(Error::Allocation(error));
    }
    output.push_str(text);
    Ok(output)
}
