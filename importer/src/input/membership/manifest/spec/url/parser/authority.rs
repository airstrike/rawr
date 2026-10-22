// Copyright 2013-2016 The rust-url developers. MIT OR Apache-2.0.

//! Filtered userinfo, borrowed-or-admitted hosts, and exact port state.

use rawr_toml::document;

use super::{
    host,
    input::Input,
    output::{Output, Set},
    Failure, Invalid, Parse, Parser, Scheme, Url,
};

pub(super) enum Text<'i> {
    Borrowed(&'i str),
    Filtered(Output),
}

impl Text<'_> {
    pub(super) fn as_str<A: document::Admission>(
        &self,
        session: &mut crate::input::membership::manifest::session::Session<'_, A>,
    ) -> crate::input::membership::manifest::Step<&str> {
        session.check()?;
        match self {
            Self::Borrowed(text) => Ok(text),
            Self::Filtered(output) => output.as_str(session),
        }
    }
}

impl<A: document::Admission> Parser<'_, '_, A> {
    pub(super) fn authority(
        mut self,
        input: Input<'_>,
        scheme: Scheme,
        scheme_end: u32,
    ) -> Parse<Url> {
        self.output.append("//", self.session)?;
        let before = self.output.len();
        let remaining = self.userinfo(input, scheme)?;
        let credentials = before != self.output.len();
        // Preserve the original component-overflow positions even though the
        // contained consumer only needs scheme/path/query/fragment offsets.
        self.offset()?;
        let (text, remaining) = self.host_text(remaining, scheme)?;
        let text = text.as_str(self.session)?;
        if scheme == Scheme::Special && text.is_empty() {
            return self.invalid(Invalid::EmptyHost);
        }
        let parsed = if scheme.special() {
            host::Host::special(text, self.session)?
        } else {
            host::Host::opaque(text, self.session)?
        }
        .map_err(|error| Failure::Invalid(error.into()))?;
        let empty =
            matches!(parsed.kind(), host::Kind::Domain(domain) if domain.as_bytes().is_empty());
        self.append_host(&parsed)?;
        self.offset()?;
        if empty && (remaining.starts_with(":") || scheme.special()) {
            return self.invalid(Invalid::EmptyHost);
        }
        let remaining = if let Some(remaining) = remaining.split_prefix(":") {
            self.port(remaining, scheme_end)?
        } else {
            remaining
        };
        if empty && credentials {
            return self.invalid(Invalid::EmptyHost);
        }
        let path_start = self.offset()?;
        let remaining = self.path_start(scheme, &mut true, remaining)?;
        self.finish(scheme, scheme_end, path_start, remaining, false)
    }

    fn userinfo<'i>(&mut self, mut input: Input<'i>, scheme: Scheme) -> Parse<Input<'i>> {
        let mut last = None;
        let mut remaining = input.clone();
        let mut count = 0usize;
        while let Some(c) = remaining.next() {
            match c {
                '@' => last = Some((count, remaining.clone())),
                '/' | '?' | '#' => break,
                '\\' if scheme.special() => break,
                _ => {}
            }
            count += 1;
        }
        let (mut count, remaining) = match last {
            None => {
                self.offset()?;
                return Ok(input);
            }
            Some((0, remaining)) => {
                if remaining.clone().next().is_some_and(|c| {
                    matches!(c, '/' | '?' | '#') || (scheme.special() && c == '\\')
                }) {
                    return self.invalid(Invalid::EmptyHost);
                }
                self.offset()?;
                return Ok(remaining);
            }
            Some(last) => last,
        };
        let mut username_end = false;
        let mut password = false;
        let mut username = false;
        while count > 0 {
            let Some((c, text)) = input.next_utf8() else {
                return self.state();
            };
            count -= 1;
            if c == ':' && !username_end {
                self.offset()?;
                username_end = true;
                if count > 0 {
                    self.output.push(':', self.session)?;
                    password = true;
                }
            } else {
                if !password {
                    username = true;
                }
                self.output.encode(text, Set::Userinfo, self.session)?;
            }
        }
        if !username_end {
            self.offset()?;
        }
        if username || password {
            self.output.push('@', self.session)?;
        }
        Ok(remaining)
    }

    pub(super) fn host_text<'i>(
        &mut self,
        mut input: Input<'i>,
        scheme: Scheme,
    ) -> Parse<(Text<'i>, Input<'i>)> {
        let source = input.remaining();
        let mut brackets = false;
        let mut ignored = false;
        let mut count = 0usize;
        let mut bytes = 0usize;
        for c in source.chars() {
            match c {
                ':' if scheme != Scheme::File && !brackets => break,
                '\\' if scheme.special() => break,
                '/' | '?' | '#' => break,
                '\t' | '\n' | '\r' => ignored = true,
                '[' => {
                    brackets = true;
                    count += 1;
                }
                ']' => {
                    brackets = false;
                    count += 1;
                }
                _ => count += 1,
            }
            bytes += c.len_utf8();
        }
        let text = if ignored {
            let mut output = Output::new();
            for c in input.by_ref().take(count) {
                output.push(c, self.session)?;
            }
            Text::Filtered(output)
        } else {
            for _ in input.by_ref().take(count) {}
            Text::Borrowed(&source[..bytes])
        };
        Ok((text, input))
    }

    pub(super) fn append_host(&mut self, host: &host::Host<'_>) -> Parse<()> {
        match host.kind() {
            host::Kind::Domain(domain) => {
                let Ok(text) = std::str::from_utf8(domain.as_bytes()) else {
                    return self.state();
                };
                self.output.append(text, self.session)?;
            }
            host::Kind::Ipv4(address) => {
                let text = address.serialize(self.session)?;
                self.output.append(text.as_str(), self.session)?;
            }
            host::Kind::Ipv6(address) => {
                let text = address.serialize(self.session)?;
                self.output.push('[', self.session)?;
                self.output.append(text.as_str(), self.session)?;
                self.output.push(']', self.session)?;
            }
        }
        Ok(())
    }

    fn port<'i>(&mut self, mut input: Input<'i>, scheme_end: u32) -> Parse<Input<'i>> {
        let mut port = 0u32;
        let mut digit_seen = false;
        while let (Some(c), remaining) = input.split_first() {
            if let Some(digit) = c.to_digit(10) {
                // Each preceding iteration proved port <= u16::MAX.
                port = port * 10 + digit;
                if port > u32::from(u16::MAX) {
                    return self.invalid(Invalid::Port);
                }
                digit_seen = true;
            } else if !matches!(c, '/' | '\\' | '?' | '#') {
                return self.invalid(Invalid::Port);
            } else {
                break;
            }
            input = remaining;
        }
        let default = match &self.output.as_str(self.session)?[..scheme_end as usize] {
            "http" | "ws" => Some(80),
            "https" | "wss" => Some(443),
            "ftp" => Some(21),
            _ => None,
        };
        if digit_seen && Some(port) != default {
            self.output.push(':', self.session)?;
            let mut buffer = [0u8; 5];
            let mut start = buffer.len();
            loop {
                start -= 1;
                buffer[start] = b'0' + (port % 10) as u8;
                port /= 10;
                if port == 0 {
                    break;
                }
            }
            let Ok(text) = std::str::from_utf8(&buffer[start..]) else {
                return self.state();
            };
            self.output.append(text, self.session)?;
        }
        Ok(input)
    }
}
