//! Semantic control flow extracted from pinned `toml::de::parser`.

use core::ops::Range;

use crate::decoder::ScalarKind;
use crate::parser::{Event, EventKind};
use crate::{ParseError, Source};

use super::storage::{self, Buffer, Builder, Shared};
use super::{
    Admission, Array, Diagnostic, Float, Integer, Key, Request, Spanned, String, Table, Tag, Value,
};

struct Input<'a> {
    events: &'a [Event],
    position: usize,
}

impl Input<'_> {
    fn next(&mut self) -> Option<Event> {
        let event = self.events.get(self.position).copied()?;
        self.position += 1;
        Some(event)
    }

    fn peek(&self, offset: usize) -> Option<EventKind> {
        self.events.get(self.position + offset).map(Event::kind)
    }

    fn whitespace(&mut self) {
        if self.peek(0) == Some(EventKind::Whitespace) {
            self.position += 1;
        }
    }
}

struct Header<'i> {
    path: Buffer<Key<'i>>,
    key: Option<Key<'i>>,
    span: Range<usize>,
    array: bool,
}

struct State<'i> {
    root: Table<'i>,
    current: Table<'i>,
    header: Option<Header<'i>>,
}

pub(super) fn document<'i, A: Admission>(
    events: &[Event],
    source: Source<'i>,
    depth: u32,
    session: &Shared<'_, A>,
) -> Option<Table<'i>> {
    if !storage::admit(session, Request::Items(1)) {
        return None;
    }
    let mut state = State {
        root: Table::default(),
        current: Table::default(),
        header: None,
    };
    let mut input = Input {
        events,
        position: 0,
    };
    while let Some(event) = input.next() {
        if !storage::live(session) {
            return None;
        }
        match event.kind() {
            EventKind::StdTableOpen | EventKind::ArrayTableOpen => {
                finish_table(&mut state, session)?;
                let header = header(event, &mut input, source, depth, session)?;
                start_table(&mut state, header, session)?;
            }
            EventKind::SimpleKey => {
                let (path, key) = key(event, &mut input, source, depth, session)?;
                input.whitespace();
                if input.next().map(|event| event.kind()) != Some(EventKind::KeyValSep) {
                    break;
                }
                input.whitespace();
                let value = value(&mut input, source, depth, session)?;
                let dotted = !path.as_slice().is_empty();
                let parent = descend(&mut state.current, path.as_slice(), dotted, false, session)?;
                if dotted && !parent.implicit {
                    duplicate(&key, None, session);
                    return None;
                }
                insert_unique(parent, key, value, session)?;
            }
            _ => {}
        }
    }
    finish_table(&mut state, session)?;
    Some(state.root)
}

fn header<'i, A: Admission>(
    open: Event,
    input: &mut Input<'_>,
    source: Source<'i>,
    depth: u32,
    session: &Shared<'_, A>,
) -> Option<Header<'i>> {
    let mut result = Header {
        path: Buffer::new(),
        key: None,
        span: open.span().start()..open.span().end(),
        array: open.kind() == EventKind::ArrayTableOpen,
    };
    while let Some(event) = input.next() {
        match event.kind() {
            EventKind::StdTableClose | EventKind::ArrayTableClose => {
                result.span.end = event.span().end();
                break;
            }
            EventKind::SimpleKey => {
                let (path, key) = key(event, input, source, depth, session)?;
                result.path = path;
                result.key = Some(key);
            }
            _ => {}
        }
    }
    Some(result)
}

fn finish_table<A: Admission>(state: &mut State<'_>, session: &Shared<'_, A>) -> Option<()> {
    let previous = core::mem::take(&mut state.current);
    let Some(header) = state.header.take() else {
        state.root = previous;
        return Some(());
    };
    let Some(key) = header.key else {
        return Some(());
    };
    if !storage::admit(session, Request::Items(1)) {
        return None;
    }
    let value = Spanned {
        span: header.span.clone(),
        value: Value::Table(previous),
    };
    let parent = descend(
        &mut state.root,
        header.path.as_slice(),
        false,
        false,
        session,
    )?;
    if header.array {
        let index = match parent.find(key.as_str()) {
            Some(index) => index,
            None => {
                if !storage::admit(session, Request::Items(1)) {
                    return None;
                }
                parent.insert(
                    copy_key(&key, session)?,
                    Spanned {
                        span: header.span,
                        value: Value::Array(Array::tables()),
                    },
                    session,
                )?
            }
        };
        let entry = parent.entry_mut(index);
        match &mut entry.1.value {
            Value::Array(array) if array.array_of_tables => {
                if !array.values.push(value, session) {
                    return None;
                }
            }
            _ => {
                duplicate(&key, Some(entry.1.span.clone()), session);
                return None;
            }
        }
    } else {
        // start_table already removed any permitted implicit predecessor.
        parent.insert(key, value, session)?;
    }
    Some(())
}

fn start_table<'i, A: Admission>(
    state: &mut State<'i>,
    header: Header<'i>,
    session: &Shared<'_, A>,
) -> Option<()> {
    if !header.array
        && let Some(key) = &header.key
    {
        let parent = descend(
            &mut state.root,
            header.path.as_slice(),
            false,
            false,
            session,
        )?;
        if let Some((old_key, old)) = parent.remove(key.as_str()) {
            match old.value {
                Value::Table(table) if table.implicit && !table.dotted => state.current = table,
                _ => {
                    duplicate(key, Some(old_key.span), session);
                    return None;
                }
            }
        }
    }
    state.current.implicit = false;
    state.current.dotted = false;
    state.header = Some(header);
    Some(())
}

fn more_key(input: &Input<'_>) -> bool {
    input.peek(0) == Some(EventKind::KeySep)
        || (input.peek(0) == Some(EventKind::Whitespace)
            && input.peek(1) == Some(EventKind::KeySep))
}

fn key<'i, A: Admission>(
    first: Event,
    input: &mut Input<'_>,
    source: Source<'i>,
    depth: u32,
    session: &Shared<'_, A>,
) -> Option<(Buffer<Key<'i>>, Key<'i>)> {
    let mut current = Some(first);
    let mut path = Buffer::new();
    let mut last = None;
    if more_key(input) {
        while let Some(event) = input.next() {
            match event.kind() {
                EventKind::SimpleKey => {
                    current = Some(event);
                    if !more_key(input) {
                        break;
                    }
                }
                EventKind::KeySep => {
                    close_key(&mut current, &mut path, &mut last, source, session)?
                }
                _ => {}
            }
        }
    }
    close_key(&mut current, &mut path, &mut last, source, session)?;
    if path.len() >= depth as usize {
        storage::invalid(
            session,
            Diagnostic {
                tag: Tag::Depth,
                span: None,
                context: None,
            },
        );
        return None;
    }
    last.map(|key| (path, key))
}

fn close_key<'i, A: Admission>(
    current: &mut Option<Event>,
    path: &mut Buffer<Key<'i>>,
    last: &mut Option<Key<'i>>,
    source: Source<'i>,
    session: &Shared<'_, A>,
) -> Option<()> {
    if let Some(event) = current.take() {
        if !storage::admit(session, Request::Items(1)) {
            return None;
        }
        let raw = source.get(event)?;
        let mut builder = Builder::new(session);
        let mut errors = |error: ParseError| storage::raw_error(session, error);
        raw.decode_key(&mut builder, &mut errors);
        if !storage::live(session) {
            return None;
        }
        let key = Key {
            text: builder.text,
            span: event.span().start()..event.span().end(),
        };
        if let Some(previous) = last.replace(key)
            && !path.push(previous, session)
        {
            return None;
        }
    }
    Some(())
}

fn value<'i, A: Admission>(
    input: &mut Input<'_>,
    source: Source<'i>,
    depth: u32,
    session: &Shared<'_, A>,
) -> Option<Spanned<Value<'i>>> {
    let event = input.next()?;
    match event.kind() {
        EventKind::InlineTableOpen => inline_table(event, input, source, depth, session),
        EventKind::ArrayOpen => array(event, input, source, depth, session),
        EventKind::Scalar => scalar(event, source, session),
        _ => None, // A raw syntax failure cannot become a document.
    }
}

fn scalar<'i, A: Admission>(
    event: Event,
    source: Source<'i>,
    session: &Shared<'_, A>,
) -> Option<Spanned<Value<'i>>> {
    if !storage::admit(session, Request::Items(1)) {
        return None;
    }
    let raw = source.get(event)?;
    let mut builder = Builder::new(session);
    let mut errors = |error: ParseError| storage::raw_error(session, error);
    let kind = raw.decode_scalar(&mut builder, &mut errors);
    if !storage::live(session) {
        return None;
    }
    let decoded = builder.text;
    let span = event.span().start()..event.span().end();
    let value = match kind {
        ScalarKind::String => Value::String(String(decoded)),
        ScalarKind::Boolean(value) => Value::Boolean(value),
        ScalarKind::DateTime => match decoded.as_str().parse::<toml_datetime::Datetime>() {
            Ok(value) => Value::DateTime(value),
            Err(_) => {
                storage::invalid(
                    session,
                    Diagnostic {
                        tag: Tag::DateTime,
                        span: Some(span),
                        context: None,
                    },
                );
                return None;
            }
        },
        ScalarKind::Float => Value::Float(Float(decoded)),
        ScalarKind::Integer(radix) => Value::Integer(Integer {
            spelling: decoded,
            radix: radix.value(),
        }),
    };
    Some(Spanned { span, value })
}

fn array<'i, A: Admission>(
    open: Event,
    input: &mut Input<'_>,
    source: Source<'i>,
    depth: u32,
    session: &Shared<'_, A>,
) -> Option<Spanned<Value<'i>>> {
    if !storage::admit(session, Request::Items(1)) {
        return None;
    }
    let mut result = Array::default();
    let mut current = None;
    let mut close = open.span();
    while let Some(event) = input.next() {
        close = event.span();
        match event.kind() {
            EventKind::InlineTableOpen => {
                current = Some(inline_table(event, input, source, depth, session)?)
            }
            EventKind::ArrayOpen => current = Some(array(event, input, source, depth, session)?),
            EventKind::Scalar => current = Some(scalar(event, source, session)?),
            EventKind::ValueSep | EventKind::ArrayClose => {
                if let Some(value) = current.take()
                    && !result.values.push(value, session)
                {
                    return None;
                }
                if event.kind() == EventKind::ArrayClose {
                    break;
                }
            }
            EventKind::Whitespace | EventKind::Comment | EventKind::Newline | EventKind::Error => {}
            _ => break,
        }
    }
    Some(Spanned {
        span: open.span().start()..close.end(),
        value: Value::Array(result),
    })
}

fn inline_table<'i, A: Admission>(
    open: Event,
    input: &mut Input<'_>,
    source: Source<'i>,
    depth: u32,
    session: &Shared<'_, A>,
) -> Option<Spanned<Value<'i>>> {
    if !storage::admit(session, Request::Items(1)) {
        return None;
    }
    let mut result = Table::marked(false, false, true);
    let mut current_key = None;
    let mut current_value = None;
    let mut close = open.span();
    while let Some(event) = input.next() {
        close = event.span();
        match event.kind() {
            EventKind::SimpleKey => current_key = Some(key(event, input, source, depth, session)?),
            EventKind::InlineTableOpen => {
                current_value = Some(inline_table(event, input, source, depth, session)?)
            }
            EventKind::ArrayOpen => {
                current_value = Some(array(event, input, source, depth, session)?)
            }
            EventKind::Scalar => current_value = Some(scalar(event, source, session)?),
            EventKind::ValueSep | EventKind::InlineTableClose => {
                if let (Some((path, key)), Some(value)) = (current_key.take(), current_value.take())
                {
                    let table = descend(&mut result, path.as_slice(), true, true, session)?;
                    if table.dotted == path.as_slice().is_empty() {
                        duplicate(&key, None, session);
                        return None;
                    }
                    insert_unique(table, key, value, session)?;
                }
                if event.kind() == EventKind::InlineTableClose {
                    break;
                }
            }
            EventKind::Whitespace
            | EventKind::Comment
            | EventKind::Newline
            | EventKind::Error
            | EventKind::KeyValSep => {}
            _ => break,
        }
    }
    Some(Spanned {
        span: open.span().start()..close.end(),
        value: Value::Table(result),
    })
}

fn copy_key<'i, A: Admission>(key: &Key<'i>, session: &Shared<'_, A>) -> Option<Key<'i>> {
    if !storage::admit(session, Request::Items(1)) {
        return None;
    }
    Some(Key {
        text: key.text.copy(session)?,
        span: key.span.clone(),
    })
}

fn descend<'t, 'i, A: Admission>(
    mut table: &'t mut Table<'i>,
    path: &[Key<'i>],
    dotted: bool,
    inline: bool,
    session: &Shared<'_, A>,
) -> Option<&'t mut Table<'i>> {
    for key in path {
        let index = match table.find(key.as_str()) {
            Some(index) => index,
            None => {
                if !storage::admit(session, Request::Items(1)) {
                    return None;
                }
                let child = Table::marked(true, dotted, inline);
                table.insert(
                    copy_key(key, session)?,
                    Spanned {
                        span: key.span.clone(),
                        value: Value::Table(child),
                    },
                    session,
                )?
            }
        };
        let entry = table.entry_mut(index);
        let old_span = entry.1.span.clone();
        match &mut entry.1.value {
            Value::Table(child) => {
                if !inline && child.inline {
                    storage::invalid(
                        session,
                        Diagnostic {
                            tag: Tag::ExtendInlineTable,
                            span: Some(key.span.clone()),
                            context: None,
                        },
                    );
                    return None;
                }
                if dotted && child.implicit {
                    child.dotted = true;
                }
                if dotted && !child.implicit {
                    duplicate(key, None, session);
                    return None;
                }
                table = child;
            }
            Value::Array(array) if !inline && array.array_of_tables => {
                match array.values.last_mut() {
                    Some(Spanned {
                        value: Value::Table(child),
                        ..
                    }) => table = child,
                    _ => {
                        wrong_type(key, old_span, session);
                        return None;
                    }
                }
            }
            _ => {
                wrong_type(key, old_span, session);
                return None;
            }
        }
    }
    Some(table)
}

fn insert_unique<'i, A: Admission>(
    table: &mut Table<'i>,
    key: Key<'i>,
    value: Spanned<Value<'i>>,
    session: &Shared<'_, A>,
) -> Option<()> {
    if let Some(index) = table.find(key.as_str()) {
        duplicate(&key, Some(table.entry(index).0.span.clone()), session);
        return None;
    }
    table.insert(key, value, session)?;
    Some(())
}

fn duplicate<A: Admission>(key: &Key<'_>, context: Option<Range<usize>>, session: &Shared<'_, A>) {
    storage::invalid(
        session,
        Diagnostic {
            tag: Tag::DuplicateKey,
            span: Some(key.span.clone()),
            context,
        },
    );
}

fn wrong_type<A: Admission>(key: &Key<'_>, context: Range<usize>, session: &Shared<'_, A>) {
    storage::invalid(
        session,
        Diagnostic {
            tag: Tag::ExtendWrongType,
            span: Some(key.span.clone()),
            context: Some(context),
        },
    );
}
