// Copyright 2013-2016 The rust-url developers. MIT OR Apache-2.0.

//! Pinned bulk path segments, dot reductions and file drive handling.

use rawr_toml::document;

use super::{input::Input, output::Set, Parse, Parser, Scheme};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Context {
    Parse,
    Setter,
    Segment,
}

impl<A: document::Admission> Parser<'_, '_, A> {
    pub(super) fn path_start<'i>(
        &mut self,
        scheme: Scheme,
        has_host: &mut bool,
        input: Input<'i>,
    ) -> Parse<Input<'i>> {
        self.path_start_in(scheme, has_host, input, Context::Parse)
    }

    pub(super) fn path_start_in<'i>(
        &mut self,
        scheme: Scheme,
        has_host: &mut bool,
        input: Input<'i>,
        context: Context,
    ) -> Parse<Input<'i>> {
        let start = self.output.len();
        let (first, remaining) = input.split_first();
        if scheme.special() {
            if !self.output.as_bytes().ends_with(b"/") {
                self.output.push('/', self.session)?;
                if matches!(first, Some('/' | '\\')) {
                    return self.path_in(scheme, has_host, start, remaining, context);
                }
            }
            return self.path_in(scheme, has_host, start, input, context);
        }
        if matches!(first, Some('?' | '#')) {
            return Ok(input);
        }
        if first.is_some() && first != Some('/') {
            self.output.push('/', self.session)?;
        }
        self.path_in(scheme, has_host, start, input, context)
    }

    fn pending(
        &mut self,
        start: &str,
        remaining: usize,
        scheme: Scheme,
        context: Context,
    ) -> Parse<()> {
        let text = &start[..start.len() - remaining];
        if !text.is_empty() {
            let set = match context {
                Context::Segment if scheme.special() => Set::SpecialPathSegment,
                Context::Segment => Set::PathSegment,
                _ => Set::Path,
            };
            self.output.encode(text, set, self.session)?;
        }
        Ok(())
    }

    pub(super) fn path<'i>(
        &mut self,
        scheme: Scheme,
        has_host: &mut bool,
        path_start: usize,
        input: Input<'i>,
    ) -> Parse<Input<'i>> {
        self.path_in(scheme, has_host, path_start, input, Context::Parse)
    }

    pub(super) fn path_in<'i>(
        &mut self,
        scheme: Scheme,
        has_host: &mut bool,
        path_start: usize,
        mut input: Input<'i>,
        context: Context,
    ) -> Parse<Input<'i>> {
        loop {
            let mut segment_start = self.output.len();
            let mut ends_with_slash = false;
            let mut start = input.remaining();
            loop {
                let before = input.clone();
                let Some(c) = input.next_raw() else {
                    self.pending(start, 0, scheme, context)?;
                    break;
                };
                match c {
                    '\t' | '\n' | '\r' => {
                        self.pending(start, before.remaining().len(), scheme, context)?;
                        start = input.remaining();
                    }
                    '/' if context != Context::Segment => {
                        self.pending(start, before.remaining().len(), scheme, context)?;
                        self.output.push('/', self.session)?;
                        ends_with_slash = true;
                        break;
                    }
                    '\\' if context != Context::Segment && scheme.special() => {
                        self.pending(start, before.remaining().len(), scheme, context)?;
                        self.output.push('/', self.session)?;
                        ends_with_slash = true;
                        break;
                    }
                    '?' | '#' if context == Context::Parse => {
                        self.pending(start, before.remaining().len(), scheme, context)?;
                        input = before;
                        break;
                    }
                    _ => {
                        if scheme == Scheme::File
                            && self.output.len() > path_start
                            && normalized_drive(&self.output.as_bytes()[path_start + 1..])
                        {
                            self.pending(start, before.remaining().len(), scheme, context)?;
                            start = before.remaining();
                            self.output.push('/', self.session)?;
                            segment_start += 1;
                        }
                    }
                }
            }
            let end = self.output.len() - usize::from(ends_with_slash);
            let segment = &self.output.as_bytes()[segment_start..end];
            match segment {
                b".." | b"%2e%2e" | b"%2e%2E" | b"%2E%2e" | b"%2E%2E" | b"%2e." | b"%2E."
                | b".%2e" | b".%2E" => {
                    self.output.truncate(segment_start, self.session)?;
                    if self.output.as_bytes().ends_with(b"/") && self.removable_slash(path_start) {
                        self.output.pop(self.session)?;
                    }
                    self.shorten(scheme, path_start)?;
                    if ends_with_slash && !self.output.as_bytes().ends_with(b"/") {
                        self.output.push('/', self.session)?;
                    }
                }
                b"." | b"%2e" | b"%2E" => {
                    self.output.truncate(segment_start, self.session)?;
                    if !self.output.as_bytes().ends_with(b"/") {
                        self.output.push('/', self.session)?;
                    }
                }
                _ => {
                    if scheme == Scheme::File && segment_start == path_start + 1 && drive(segment) {
                        let letter = char::from(segment[0]);
                        self.output.truncate(segment_start, self.session)?;
                        self.output.push(letter, self.session)?;
                        self.output.push(':', self.session)?;
                        if ends_with_slash {
                            self.output.push('/', self.session)?;
                        }
                        *has_host = false;
                    }
                }
            }
            if !ends_with_slash {
                break;
            }
        }
        if scheme == Scheme::File {
            // Same final path as split_off + '/' + trim_start_matches('/'),
            // moving within admitted storage without an intermediate owner.
            let leading = self.output.as_bytes()[path_start..]
                .iter()
                .take_while(|&&b| b == b'/')
                .count();
            if leading == 0 {
                self.output.insert(path_start, "/", self.session)?;
            } else if leading > 1 {
                self.output
                    .remove(path_start..path_start + leading - 1, self.session)?;
            }
        }
        Ok(input)
    }

    fn removable_slash(&self, path_start: usize) -> bool {
        let text = self.output.as_bytes();
        text[..text.len() - 1]
            .iter()
            .rposition(|&b| b == b'/')
            .is_some_and(|position| position >= path_start && !path_drive(&text[position..]))
    }

    pub(super) fn shorten(&mut self, scheme: Scheme, path_start: usize) -> Parse<()> {
        if self.output.len() == path_start
            || (scheme == Scheme::File && normalized_drive(&self.output.as_bytes()[path_start..]))
        {
            return Ok(());
        }
        let Some(slash) = self.output.as_bytes()[path_start..]
            .iter()
            .rposition(|&b| b == b'/')
        else {
            return self.state();
        };
        let segment = path_start + slash + 1;
        if !(scheme == Scheme::File && normalized_drive(&self.output.as_bytes()[segment..])) {
            self.output.truncate(segment, self.session)?;
        }
        Ok(())
    }

    pub(super) fn opaque<'i>(&mut self, mut input: Input<'i>) -> Parse<Input<'i>> {
        loop {
            let before = input.clone();
            match input.next_utf8() {
                Some(('?' | '#', _)) => return Ok(before),
                Some((_, text)) => self.output.encode(text, Set::Controls, self.session)?,
                None => return Ok(input),
            }
        }
    }
}

pub(super) fn drive(bytes: &[u8]) -> bool {
    bytes.len() == 2 && starts_drive(bytes)
}

fn normalized_drive(bytes: &[u8]) -> bool {
    drive(bytes) && bytes[1] == b':'
}

fn starts_drive(bytes: &[u8]) -> bool {
    bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && matches!(bytes[1], b':' | b'|')
        && (bytes.len() == 2 || matches!(bytes[2], b'/' | b'\\' | b'?' | b'#'))
}

fn path_drive(bytes: &[u8]) -> bool {
    bytes
        .first()
        .is_some_and(|b| matches!(b, b'/' | b'\\' | b'?' | b'#'))
        && starts_drive(&bytes[1..])
}
