// Copyright the gitoxide contributors. Parser: MIT OR Apache-2.0.
// BOM recognition is derived from unicode-bom 2.0.3, licensed Apache-2.0.
//! gix-config 0.58.0 event parser; complete owned events, original failure cursor.

use super::{own, storage, Error, Event, Front, Header, Name, Session, Step};
use crate::input::membership::manifest::Stop;
use rawr_toml::document::Admission;
use std::borrow::Cow;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Node {
    Header,
    Name,
    Value,
}
#[derive(Debug)]
// gix-config's parse error; read only through Debug.
#[allow(dead_code)]
pub(in super::super) struct Failure {
    pub(super) line: usize,
    pub(super) node: Node,
    pub(super) remaining: Vec<u8>,
}

pub(super) struct Section {
    pub(super) header: Header<'static>,
    pub(super) events: storage::Vector<Event<'static>>,
}
pub(super) struct Parsed {
    pub(super) front: Front,
    pub(super) sections: storage::Vector<Section>,
}
struct Collect {
    header: Option<Header<'static>>,
    events: storage::Vector<Event<'static>>,
    front: Front,
    sections: storage::Vector<Section>,
}
enum Abort {
    Syntax,
    Stop(Stop),
}
impl From<Stop> for Abort {
    fn from(stop: Stop) -> Self {
        Self::Stop(stop)
    }
}
type Result<T> = std::result::Result<T, Abort>;

impl Collect {
    fn emit<A: Admission>(&mut self, event: Event<'_>, session: &mut Session<'_, A>) -> Result<()> {
        match event {
            Event::Header(header) => {
                match self.header.take() {
                    None => self.front = front(std::mem::take(&mut self.events), session)?,
                    Some(header) => self.sections.push(
                        Section {
                            header,
                            events: std::mem::take(&mut self.events),
                        },
                        session,
                    )?,
                }
                self.header = Some(header.owned(session)?);
            }
            event => self.events.push(event.owned(session)?, session)?,
        }
        Ok(())
    }
}

fn front<A: Admission>(
    events: storage::Vector<Event<'static>>,
    session: &mut Session<'_, A>,
) -> Step<Front> {
    let mut result = Front::new();
    storage::small_reserve(&mut result, events.len(), session)?;
    session.items(events.len())?;
    result.extend(events.into_iter());
    Ok(result)
}

impl Header<'_> {
    pub(super) fn owned<A: Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<Header<'static>> {
        Ok(Header {
            name: Name(Cow::Owned(own(&self.name.0, session)?)),
            separator: self
                .separator
                .as_ref()
                .map(|s| own(s, session).map(Cow::Owned))
                .transpose()?,
            subsection: self
                .subsection
                .as_ref()
                .map(|s| own(s, session).map(Cow::Owned))
                .transpose()?,
        })
    }
}
impl Event<'_> {
    fn owned<A: Admission>(&self, session: &mut Session<'_, A>) -> Step<Event<'static>> {
        Ok(match self {
            Self::Comment { tag, text } => Event::Comment {
                tag: *tag,
                text: Cow::Owned(own(text, session)?),
            },
            Self::Header(header) => Event::Header(header.owned(session)?),
            Self::Name(name) => Event::Name(Name(Cow::Owned(own(&name.0, session)?))),
            Self::Separator => Event::Separator,
            Self::Value(value) => Event::Value(Cow::Owned(own(value, session)?)),
            Self::NotDone(value) => Event::NotDone(Cow::Owned(own(value, session)?)),
            Self::Done(value) => Event::Done(Cow::Owned(own(value, session)?)),
            Self::Newline(value) => Event::Newline(Cow::Owned(own(value, session)?)),
            Self::Whitespace(value) => Event::Whitespace(Cow::Owned(own(value, session)?)),
        })
    }
}

pub(super) fn parse<A: Admission>(
    input: &[u8],
    session: &mut Session<'_, A>,
) -> Step<std::result::Result<Parsed, Failure>> {
    let mut output = Collect {
        header: None,
        events: storage::Vector::with_capacity(256, session)?,
        front: Front::new(),
        sections: storage::Vector::new(),
    };
    session.items(4)?;
    let mut rest = &input[bom(input)..];
    let mut node = Node::Header;
    let result = grammar(&mut rest, &mut node, &mut output, session);
    match result {
        Err(Abort::Stop(stop)) => return Err(stop),
        Err(Abort::Syntax) => {
            session.items(input.len() - rest.len())?;
            let line = input[..input.len() - rest.len()]
                .iter()
                .filter(|b| **b == b'\n')
                .count();
            let remaining = own(rest, session)?;
            return Ok(Err(Failure {
                line,
                node,
                remaining,
            }));
        }
        Ok(()) => {}
    }
    match output.header {
        None => output.front = front(output.events, session)?,
        Some(header) => output.sections.push(
            Section {
                header,
                events: output.events,
            },
            session,
        )?,
    }
    Ok(Ok(Parsed {
        front: output.front,
        sections: output.sections,
    }))
}

fn grammar<A: Admission>(
    input: &mut &[u8],
    node: &mut Node,
    output: &mut Collect,
    session: &mut Session<'_, A>,
) -> Result<()> {
    loop {
        session.items(1)?;
        let before = input.len();
        if let Some(event) = comment(input, session)? {
            output.emit(event, session)?;
        } else if let Some(bytes) = spaces(input, session)? {
            output.emit(Event::Whitespace(Cow::Borrowed(bytes)), session)?;
        } else if let Some(bytes) = newlines(input, session)? {
            output.emit(Event::Newline(Cow::Borrowed(bytes)), session)?;
        } else if !input.starts_with(b"[") {
            *node = Node::Header;
            pair(input, node, output, session)?;
        }
        if input.len() == before {
            break;
        }
    }
    if input.is_empty() {
        return Ok(());
    }
    *node = Node::Header;
    while !input.is_empty() {
        let header = header(input, session)?;
        output.emit(Event::Header(header), session)?;
        loop {
            session.items(1)?;
            let before = input.len();
            if let Some(bytes) = spaces(input, session)? {
                output.emit(Event::Whitespace(Cow::Borrowed(bytes)), session)?;
            }
            if let Some(bytes) = newlines(input, session)? {
                output.emit(Event::Newline(Cow::Borrowed(bytes)), session)?;
            }
            pair(input, node, output, session)?;
            if let Some(event) = comment(input, session)? {
                output.emit(event, session)?;
            }
            if input.len() == before {
                break;
            }
        }
    }
    Ok(())
}

fn comment<'a, A: Admission>(
    input: &mut &'a [u8],
    session: &mut Session<'_, A>,
) -> Result<Option<Event<'a>>> {
    session.items(1)?;
    let Some((&tag, rest)) = input.split_first() else {
        return Ok(None);
    };
    if tag != b';' && tag != b'#' {
        return Ok(None);
    }
    session.items(rest.len())?;
    let end = rest.iter().position(|b| *b == b'\n').unwrap_or(rest.len());
    *input = &rest[end..];
    Ok(Some(Event::Comment {
        tag,
        text: Cow::Borrowed(&rest[..end]),
    }))
}

fn spaces<'a, A: Admission>(
    input: &mut &'a [u8],
    session: &mut Session<'_, A>,
) -> Result<Option<&'a [u8]>> {
    let mut count = 0;
    for &byte in input.iter() {
        session.items(1)?;
        if byte != b' ' && byte != b'\t' {
            break;
        }
        count += 1;
    }
    if count == 0 {
        return Ok(None);
    }
    let (bytes, rest) = input.split_at(count);
    *input = rest;
    Ok(Some(bytes))
}
fn newlines<'a, A: Admission>(
    input: &mut &'a [u8],
    session: &mut Session<'_, A>,
) -> Result<Option<&'a [u8]>> {
    let mut count = 0;
    while count < input.len() {
        session.items(2)?;
        if input[count..].starts_with(b"\r\n") {
            count += 2;
        } else if input[count] == b'\n' {
            count += 1;
        } else {
            break;
        }
    }
    if count == 0 {
        return Ok(None);
    }
    let (bytes, rest) = input.split_at(count);
    *input = rest;
    Ok(Some(bytes))
}

fn header<'a, A: Admission>(
    input: &mut &'a [u8],
    session: &mut Session<'_, A>,
) -> Result<Header<'a>> {
    session.items(1)?;
    let mut rest = input.strip_prefix(b"[").ok_or(Abort::Syntax)?;
    let original = rest;
    let mut length = 0;
    for &byte in rest {
        session.items(1)?;
        if !(byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'.') {
            break;
        }
        length += 1;
    }
    let name = &original[..length];
    rest = &original[length..];
    if let Some(tail) = rest.strip_prefix(b"]") {
        if name.is_empty() {
            return Err(Abort::Syntax);
        }
        *input = tail;
        session.items(name.len())?;
        return Ok(match name.iter().position(|b| *b == b'.') {
            Some(at) => Header {
                name: Name(Cow::Borrowed(&name[..at])),
                separator: Some(Cow::Borrowed(&name[at..at + 1])),
                subsection: Some(Cow::Borrowed(&name[at + 1..])),
            },
            None => Header {
                name: Name(Cow::Borrowed(name)),
                separator: None,
                subsection: None,
            },
        });
    }
    let separator = spaces(&mut rest, session)?.ok_or(Abort::Syntax)?;
    rest = rest.strip_prefix(b"\"").ok_or(Abort::Syntax)?;
    let subsection = subsection(&mut rest, session)?;
    rest = rest.strip_prefix(b"\"]").ok_or(Abort::Syntax)?;
    *input = rest;
    Ok(Header {
        name: Name(Cow::Borrowed(name)),
        separator: Some(Cow::Borrowed(separator)),
        subsection: Some(subsection),
    })
}

fn subsection<'a, A: Admission>(
    input: &mut &'a [u8],
    session: &mut Session<'_, A>,
) -> Result<Cow<'a, [u8]>> {
    let original = *input;
    let mut rest = *input;
    let mut output: Option<storage::Vector<u8>> = None;
    let mut borrowed = 0;
    while let Some(&byte) = rest.first() {
        session.items(1)?;
        match byte {
            b'"' => break,
            b'\n' => return Err(Abort::Syntax),
            b'\\' => {
                let &escaped = rest.get(1).ok_or(Abort::Syntax)?;
                if escaped == b'\n' {
                    return Err(Abort::Syntax);
                }
                if output.is_none() {
                    let mut out = storage::Vector::with_capacity(borrowed, session)?;
                    out.extend_copy(&original[..borrowed], session)?;
                    output = Some(out);
                }
                let Some(out) = output.as_mut() else {
                    return session.fail(Error::State).map_err(Abort::Stop);
                };
                out.push(escaped, session)?;
                rest = &rest[2..];
                borrowed = original.len() - rest.len();
            }
            _ => {
                if let Some(out) = output.as_mut() {
                    out.push(byte, session)?;
                }
                rest = &rest[1..];
                borrowed = original.len() - rest.len();
            }
        }
    }
    *input = rest;
    Ok(match output {
        Some(out) => Cow::Owned(out.into_values()),
        None => Cow::Borrowed(&original[..borrowed]),
    })
}

fn pair<A: Admission>(
    input: &mut &[u8],
    node: &mut Node,
    output: &mut Collect,
    session: &mut Session<'_, A>,
) -> Result<()> {
    *node = Node::Name;
    session.items(1)?;
    if !input.first().is_some_and(u8::is_ascii_alphabetic) {
        return Ok(());
    }
    let mut length = 0;
    for byte in input.iter() {
        session.items(1)?;
        if !(byte.is_ascii_alphanumeric() || *byte == b'-') {
            break;
        }
        length += 1;
    }
    let (name, rest) = input.split_at(length);
    *input = rest;
    output.emit(Event::Name(Name(Cow::Borrowed(name))), session)?;
    if let Some(bytes) = spaces(input, session)? {
        output.emit(Event::Whitespace(Cow::Borrowed(bytes)), session)?;
    }
    *node = Node::Value;
    if let Some(rest) = input.strip_prefix(b"=") {
        *input = rest;
        output.emit(Event::Separator, session)?;
        if let Some(bytes) = spaces(input, session)? {
            output.emit(Event::Whitespace(Cow::Borrowed(bytes)), session)?;
        }
        value(input, output, session)
    } else {
        output.emit(Event::Value(Cow::Borrowed(b"")), session)
    }
}

fn value<A: Admission>(
    input: &mut &[u8],
    output: &mut Collect,
    session: &mut Session<'_, A>,
) -> Result<()> {
    let bytes = *input;
    let mut cursor = 0;
    let mut start = 0;
    let mut end = None;
    let mut quoted = false;
    let mut partial = false;
    while cursor < bytes.len() {
        session.items(1)?;
        match bytes[cursor] {
            b'\n' => {
                end = Some(cursor);
                break;
            }
            b';' | b'#' if !quoted => {
                end = Some(cursor);
                break;
            }
            b'\\' => {
                let escape = cursor;
                cursor += 1;
                let mut consumed = 1;
                let Some(mut byte) = bytes.get(cursor).copied() else {
                    output.emit(
                        Event::NotDone(Cow::Borrowed(&bytes[start..escape])),
                        session,
                    )?;
                    output.emit(Event::Done(Cow::Borrowed(b"")), session)?;
                    *input = &[];
                    return Ok(());
                };
                if byte == b'\r' {
                    cursor += 1;
                    byte = *bytes.get(cursor).ok_or(Abort::Syntax)?;
                    if byte != b'\n' {
                        return Err(Abort::Syntax);
                    }
                    consumed += 1;
                }
                match byte {
                    b'\n' => {
                        partial = true;
                        output.emit(
                            Event::NotDone(Cow::Borrowed(&bytes[start..escape])),
                            session,
                        )?;
                        output.emit(
                            Event::Newline(Cow::Borrowed(
                                &bytes[escape + 1..escape + 1 + consumed],
                            )),
                            session,
                        )?;
                        cursor += 1;
                        start = cursor;
                        end = None;
                    }
                    b'n' | b't' | b'\\' | b'b' | b'"' => cursor += 1,
                    _ => return Err(Abort::Syntax),
                }
            }
            b'"' => {
                quoted = !quoted;
                cursor += 1;
            }
            _ => cursor += 1,
        }
    }
    if quoted {
        return Err(Abort::Syntax);
    }
    let end = end.unwrap_or(cursor);
    if end == start {
        output.emit(Event::Value(Cow::Borrowed(b"")), session)?;
        *input = &bytes[cursor..];
        return Ok(());
    }
    let mut trimmed = start;
    for (index, byte) in bytes[start..end].iter().enumerate().rev() {
        session.items(1)?;
        if !byte.is_ascii_whitespace() {
            trimmed = start + index + 1;
            break;
        }
    }
    let bytes_value = Cow::Borrowed(&bytes[start..trimmed]);
    output.emit(
        if partial {
            Event::Done(bytes_value)
        } else {
            Event::Value(bytes_value)
        },
        session,
    )?;
    *input = &bytes[trimmed..];
    Ok(())
}

// unicode-bom 2.0.3's byte-prefix precedence, without a heap or text conversion.
fn bom(input: &[u8]) -> usize {
    if input.starts_with(&[0, 0, 0xfe, 0xff])
        || input.starts_with(&[0xff, 0xfe, 0, 0])
        || input.starts_with(&[0x84, 0x31, 0x95, 0x33])
        || input.starts_with(&[0xdd, 0x73, 0x66, 0x73])
        || (input.starts_with(&[0x2b, 0x2f, 0x76])
            && input
                .get(3)
                .is_some_and(|b| matches!(b, 0x38 | 0x39 | 0x2b | 0x2f)))
    {
        4
    } else if input.starts_with(&[0xef, 0xbb, 0xbf])
        || input.starts_with(&[0x0e, 0xfe, 0xff])
        || input.starts_with(&[0xf7, 0x64, 0x4c])
        || input.starts_with(&[0xfb, 0xee, 0x28])
    {
        3
    } else if input.starts_with(&[0xfe, 0xff]) || input.starts_with(&[0xff, 0xfe]) {
        2
    } else {
        0
    }
}
