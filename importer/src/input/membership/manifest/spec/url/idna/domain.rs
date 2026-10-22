// Copyright The rust-url developers. MIT OR Apache-2.0.
//! Complete fixed-entry domain processing before writing the ASCII result.

use rawr_toml::document;
use utf8_iter::Utf8CharsEx;

use crate::input::membership::manifest::{session, storage};

use super::super::punycode;
use super::{ascii, buffer, invalid, normalize, properties, Work};

#[derive(Clone, Copy)]
pub(super) enum Label<'input> {
    Other,
    Ascii(&'input [u8]),
    Punycode(&'input [u8]),
}

pub(super) struct Domain<'input> {
    pub(super) input: &'input [u8],
    pub(super) prefix: usize,
    pub(super) characters: buffer::Characters,
    pub(super) labels: storage::Buffer<Label<'input>>,
    data: idna_adapter::Adapter,
}

impl<'input> Domain<'input> {
    pub(super) fn interpret<A: document::Admission>(
        input: &'input [u8],
        session: &mut session::Session<'_, A>,
    ) -> Work<Self> {
        session.check()?;
        let mut output = Self {
            input,
            prefix: 0,
            characters: buffer::Characters::new(),
            labels: storage::Buffer::new(),
            data: idna_adapter::Adapter::new(),
        };
        let mut recent = 0;
        for (index, &byte) in input.iter().enumerate() {
            if byte.is_ascii_lowercase() {
                continue;
            }
            if byte == b'.' {
                recent = index + 1;
                continue;
            }
            output.prefix = recent;
            output.tail(&input[recent..], session)?;
            return Ok(output);
        }
        output.prefix = input.len();
        Ok(output)
    }

    fn tail<A: document::Admission>(
        &mut self,
        tail: &'input [u8],
        session: &mut session::Session<'_, A>,
    ) -> Work<()> {
        let mut seen = false;
        let mut prefix = true;
        for label in tail.split(|byte| *byte == b'.') {
            if prefix && ascii::passthrough(label) {
                if seen {
                    self.prefix += 1;
                }
                seen = true;
                self.prefix += label.len();
                continue;
            }
            if seen {
                if prefix {
                    self.prefix += 1;
                } else {
                    self.characters.push('.', session)?;
                }
            }
            seen = true;
            prefix = false;
            let start = self.characters.len();
            if label.is_empty() {
                self.labels.push(Label::Ascii(label), session)?;
                continue;
            }
            let (ascii, non_ascii) = ascii::split(label);
            let plain = if non_ascii.is_empty() {
                if ascii::prefix(ascii) {
                    if ascii.last() == Some(&b'-') || ascii.len() - 4 > 2000 {
                        return invalid(session);
                    }
                    let mut decoder = punycode::Decoder::new();
                    let decoded = match decoder.bytes(&ascii[4..], session)? {
                        Ok(decoded) => decoded,
                        Err(_) => return invalid(session),
                    };
                    let mut label_buffer = buffer::Characters::new();
                    for c in decoded {
                        label_buffer.push(c, session)?;
                    }
                    self.decoded(start, label_buffer.as_slice(), session)?;
                    properties::label(
                        &self.data,
                        &self.characters.as_slice()[start..],
                        properties::Checks {
                            combining: true,
                            context: true,
                        },
                        session,
                    )?;
                    self.labels.push(Label::Punycode(label), session)?;
                    continue;
                }
                true
            } else {
                false
            };
            for &byte in ascii {
                let c = ascii::byte(byte);
                if c == '\u{fffd}' {
                    return invalid(session);
                }
                self.characters.push(c, session)?;
            }
            if plain {
                // Hyphens::Allow omits the source's optional hyphen checks.
                self.labels.push(Label::Ascii(label), session)?;
                continue;
            }
            self.labels.push(Label::Other, session)?;
            self.mapped(start, ascii.is_empty(), non_ascii, session)?;
        }
        properties::bidi(&self.data, self.characters.as_slice(), session)
    }

    fn mapped<A: document::Admission>(
        &mut self,
        mut start: usize,
        combining: bool,
        input: &[u8],
        session: &mut session::Session<'_, A>,
    ) -> Work<()> {
        let mut checks = properties::Checks {
            combining,
            context: !input.is_empty(),
        };
        let mut mapping = normalize::map_normalize(input.chars(), session)?;
        loop {
            let next = mapping.next(session)?.map(|c| ascii::unicode(c, false));
            match next {
                None | Some('.') => {
                    if self.characters.as_slice()[start..].starts_with(&['x', 'n', '-', '-']) {
                        let suffix = &self.characters.as_slice()[start + 4..];
                        if suffix.iter().any(|c| !c.is_ascii()) {
                            return invalid(session);
                        }
                        if self.characters.as_slice().last() == Some(&'-') {
                            return invalid(session);
                        }
                        if suffix.len() > 2000 {
                            return invalid(session);
                        }
                        let mut label_buffer = buffer::Characters::new();
                        {
                            let mut decoder = punycode::Decoder::new();
                            let decoded = match decoder.chars(suffix, session)? {
                                Ok(decoded) => decoded,
                                Err(_) => return invalid(session),
                            };
                            for c in decoded {
                                label_buffer.push(c, session)?;
                            }
                        }
                        checks = properties::Checks {
                            combining: true,
                            context: true,
                        };
                        self.characters.truncate(start);
                        self.decoded(start, label_buffer.as_slice(), session)?;
                    }
                    properties::label(
                        &self.data,
                        &self.characters.as_slice()[start..],
                        checks,
                        session,
                    )?;
                    if next.is_none() {
                        break;
                    }
                    self.characters.push('.', session)?;
                    start = self.characters.len();
                    checks = properties::Checks {
                        combining: true,
                        context: true,
                    };
                    self.labels.push(Label::Other, session)?;
                }
                Some(c) => {
                    if c == '\u{fffd}' {
                        return invalid(session);
                    }
                    self.characters.push(c, session)?;
                }
            }
        }
        Ok(())
    }

    fn decoded<A: document::Admission>(
        &mut self,
        start: usize,
        input: &[char],
        session: &mut session::Session<'_, A>,
    ) -> Work<()> {
        let mut normalized = normalize::normalize_validate(input.iter().copied(), session)?;
        while let Some(c) = normalized.next(session)? {
            let c = ascii::unicode(c, true);
            if c == '\u{fffd}' {
                return invalid(session);
            }
            self.characters.push(c, session)?;
        }
        // Preserve the source's zip comparison exactly; it does not append a
        // new equal-length check after normalize_validate has completed.
        if self.characters.as_slice()[start..]
            .iter()
            .zip(input)
            .any(|(normalized, decoded)| normalized != decoded)
        {
            return invalid(session);
        }
        Ok(())
    }
}
