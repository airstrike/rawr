// Copyright The rust-url developers. MIT OR Apache-2.0.
//! Delayed ASCII sink writes and the pinned extended-passthrough path.

use rawr_toml::document;

use crate::input::membership::manifest::{session, storage, Error, Step};

use super::super::punycode;
use super::{domain, invalid, Work};

pub(in crate::input::membership::manifest::spec::url) enum Bytes<'input> {
    Borrowed(&'input [u8]),
    Owned(storage::Buffer<u8>),
}

impl Bytes<'_> {
    pub(in crate::input::membership::manifest::spec::url) fn as_slice(&self) -> &[u8] {
        match self {
            Self::Borrowed(value) => value,
            Self::Owned(value) => value.as_slice(),
        }
    }
}

struct Sink {
    bytes: storage::Buffer<u8>,
}

impl Sink {
    fn push<A: document::Admission>(
        &mut self,
        byte: u8,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        let Some(length) = self.bytes.len().checked_add(1) else {
            return session.fail(Error::Overflow);
        };
        session.string(length)?;
        self.bytes.push(byte, session)?;
        Ok(())
    }

    fn write<A: document::Admission>(
        &mut self,
        bytes: &[u8],
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        for &byte in bytes {
            self.push(byte, session)?;
        }
        Ok(())
    }
}

impl<'input> domain::Domain<'input> {
    pub(super) fn output<A: document::Admission>(
        self,
        session: &mut session::Session<'_, A>,
    ) -> Work<Bytes<'input>> {
        session.check()?;
        if self.prefix == self.input.len() {
            return Ok(Bytes::Borrowed(self.input));
        }
        let mut sink = Sink {
            bytes: storage::Buffer::new(),
        };
        let mut markers = self.labels.as_slice().iter();
        let mut prefix = self.prefix;
        let mut flushed = false;
        let mut seen = false;
        for label in self.characters.as_slice().split(|c| *c == '.') {
            let Some(marker) = markers.next() else {
                return session.fail(Error::State).map_err(Into::into);
            };
            if seen {
                if flushed {
                    sink.push(b'.', session)?;
                } else {
                    prefix += 1;
                    if prefix == self.input.len() {
                        return Ok(Bytes::Borrowed(self.input));
                    }
                }
            }
            seen = true;
            if let domain::Label::Ascii(mixed) = marker {
                if let Some(first) = mixed.iter().position(u8::is_ascii_uppercase) {
                    let (head, tail) = mixed.split_at(first);
                    let write = if flushed {
                        head
                    } else {
                        flushed = true;
                        prefix += head.len();
                        &self.input[..prefix]
                    };
                    sink.write(write, session)?;
                    for byte in tail {
                        sink.push(byte.to_ascii_lowercase(), session)?;
                    }
                } else if flushed {
                    sink.write(mixed, session)?;
                } else {
                    prefix += mixed.len();
                    if prefix == self.input.len() {
                        return Ok(Bytes::Borrowed(self.input));
                    }
                }
                continue;
            }
            if label.iter().all(char::is_ascii) {
                if !flushed {
                    flushed = true;
                    sink.write(&self.input[..prefix], session)?;
                }
                for &c in label {
                    sink.push(c as u8, session)?;
                }
            } else if let domain::Label::Punycode(mixed) = marker {
                if let Some(first) = mixed.iter().position(u8::is_ascii_uppercase) {
                    let (head, tail) = mixed.split_at(first);
                    let write = if flushed {
                        head
                    } else {
                        flushed = true;
                        prefix += head.len();
                        &self.input[..prefix]
                    };
                    sink.write(write, session)?;
                    for byte in tail {
                        sink.push(byte.to_ascii_lowercase(), session)?;
                    }
                } else if flushed {
                    sink.write(mixed, session)?;
                } else {
                    prefix += mixed.len();
                    if prefix == self.input.len() {
                        return Ok(Bytes::Borrowed(self.input));
                    }
                }
            } else {
                if !flushed {
                    flushed = true;
                    sink.write(&self.input[..prefix], session)?;
                }
                sink.write(b"xn--", session)?;
                let encoded = match punycode::encode(label, session)? {
                    Ok(encoded) => encoded,
                    Err(_) => return invalid(session),
                };
                sink.write(encoded.as_bytes(), session)?;
            }
        }
        if markers.next().is_some() {
            return session.fail(Error::State).map_err(Into::into);
        }
        Ok(Bytes::Owned(sink.bytes))
    }
}
