//! Cargo's final recognized Git query pair determines the reference.

use rawr_toml::document;
use std::alloc::Layout;
use std::borrow::Cow;

use super::super::{session, Error, Step};

/// The decoded reference selected by the pinned Git query computation.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(in super::super) struct Reference<'query> {
    kind: Kind,
    value: Cow<'query, str>,
}

/// SourceKind's Git reference discriminants in their declared comparison order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(in super::super) enum Kind {
    Tag,
    Branch,
    Rev,
    DefaultBranch,
}

/// Percent decoding leaves malformed escapes intact and maps plus to space.
#[derive(Clone)]
struct Bytes<'query> {
    remaining: &'query [u8],
}

impl Reference<'_> {
    pub(super) fn into_owned<A: document::Admission>(
        self,
        session: &mut session::Session<'_, A>,
    ) -> Step<Reference<'static>> {
        session.check()?;
        let value = match self.value {
            Cow::Owned(value) => value,
            Cow::Borrowed(value) => {
                let mut bytes = allocate(value.len(), session)?;
                bytes.extend_from_slice(value.as_bytes());
                match String::from_utf8(bytes) {
                    Ok(value) => value,
                    Err(_) => return session.fail(Error::State),
                }
            }
        };
        Ok(Reference {
            kind: self.kind,
            value: Cow::Owned(value),
        })
    }

    pub(in super::super) fn kind(&self) -> Kind {
        self.kind
    }

    pub(in super::super) fn value(&self) -> &str {
        &self.value
    }
}

/// Interpret query pairs using Cargo's ordered, last-recognized-pair rule.
pub(in super::super) fn parse<'query, A: document::Admission>(
    query: &'query str,
    session: &mut session::Session<'_, A>,
) -> Step<Reference<'query>> {
    session.check()?;
    let mut selected = None;
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let decoded = Bytes {
            remaining: key.as_bytes(),
        };
        // Lossy UTF-8 cannot turn a non-ASCII byte into an ASCII keyword.
        let kind = if decoded.clone().eq(b"branch".iter().copied())
            || decoded.clone().eq(b"ref".iter().copied())
        {
            Kind::Branch
        } else if decoded.clone().eq(b"rev".iter().copied()) {
            Kind::Rev
        } else if decoded.eq(b"tag".iter().copied()) {
            Kind::Tag
        } else {
            continue;
        };
        selected = Some((kind, value));
    }
    let Some((kind, value)) = selected else {
        return Ok(Reference {
            kind: Kind::DefaultBranch,
            value: Cow::Borrowed(""),
        });
    };
    Ok(Reference {
        kind,
        value: decode(value, session)?,
    })
}

impl Iterator for Bytes<'_> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        let (&first, rest) = self.remaining.split_first()?;
        self.remaining = rest;
        if first == b'%'
            && let [high, low, rest @ ..] = rest
            && let (Some(high), Some(low)) = (hex(*high), hex(*low))
        {
            self.remaining = rest;
            return Some((high << 4) | low);
        }
        Some(if first == b'+' { b' ' } else { first })
    }
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn decode<'query, A: document::Admission>(
    value: &'query str,
    session: &mut session::Session<'_, A>,
) -> Step<Cow<'query, str>> {
    let decoded = Bytes {
        remaining: value.as_bytes(),
    };
    if decoded.clone().eq(value.bytes()) {
        return Ok(Cow::Borrowed(value));
    }
    let length = decoded.clone().count();
    let mut owned = allocate(length, session)?;
    owned.extend(decoded);
    match String::from_utf8(owned) {
        Ok(value) => Ok(Cow::Owned(value)),
        Err(error) => {
            let bytes = error.into_bytes();
            let length = bytes.utf8_chunks().try_fold(0usize, |length, chunk| {
                length.checked_add(chunk.valid().len()).and_then(|length| {
                    length.checked_add(if chunk.invalid().is_empty() { 0 } else { 3 })
                })
            });
            let Some(length) = length else {
                return session.fail(Error::Overflow);
            };
            let mut repaired = allocate(length, session)?;
            for chunk in bytes.utf8_chunks() {
                repaired.extend_from_slice(chunk.valid().as_bytes());
                if !chunk.invalid().is_empty() {
                    repaired.extend_from_slice("\u{fffd}".as_bytes());
                }
            }
            match String::from_utf8(repaired) {
                Ok(value) => Ok(Cow::Owned(value)),
                Err(_) => session.fail(Error::State),
            }
        }
    }
}

fn allocate<A: document::Admission>(
    length: usize,
    session: &mut session::Session<'_, A>,
) -> Step<Vec<u8>> {
    let Ok(layout) = Layout::array::<u8>(length) else {
        return session.fail(Error::Overflow);
    };
    session.string(length)?;
    session.items(1)?;
    session.admit(document::Request::Allocation {
        bytes: layout.size(),
        alignment: layout.align(),
    })?;
    let mut bytes = Vec::new();
    if let Err(error) = bytes.try_reserve_exact(length) {
        return session.fail(Error::Allocation(error));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests;
