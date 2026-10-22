//! Actual raw events, including recovery emissions, are admitted individually.
//! Unlike the raw FnMut receiver blanket impl, opens return false after failure.
//! This makes subsequent raw nested recovery use its iterative skipping path.

use crate::decoder::Encoding;
use crate::parser::{Event, EventKind, EventReceiver};
use crate::{ErrorSink, Span};

use super::storage::{Buffer, Shared};
use super::Admission;

pub(super) struct Receiver<'e, 's, 'a, A: Admission> {
    pub(super) events: &'e mut Buffer<Event>,
    pub(super) session: &'s Shared<'a, A>,
}

impl<A: Admission> Receiver<'_, '_, '_, A> {
    fn emit(&mut self, kind: EventKind, encoding: Option<Encoding>, span: Span) -> bool {
        self.events
            .push(Event::new_unchecked(kind, encoding, span), self.session)
    }
}

macro_rules! events {
    ($($method:ident => $kind:ident),* $(,)?) => {$(
        fn $method(&mut self, span: Span, _error: &mut dyn ErrorSink) {
            self.emit(EventKind::$kind, None, span);
        }
    )*};
}

impl<A: Admission> EventReceiver for Receiver<'_, '_, '_, A> {
    events! {
        std_table_open => StdTableOpen,
        std_table_close => StdTableClose,
        array_table_open => ArrayTableOpen,
        array_table_close => ArrayTableClose,
        inline_table_close => InlineTableClose,
        array_close => ArrayClose,
        key_sep => KeySep,
        key_val_sep => KeyValSep,
        value_sep => ValueSep,
        whitespace => Whitespace,
        comment => Comment,
        newline => Newline,
        error => Error,
    }

    fn inline_table_open(&mut self, span: Span, _error: &mut dyn ErrorSink) -> bool {
        self.emit(EventKind::InlineTableOpen, None, span)
    }

    fn array_open(&mut self, span: Span, _error: &mut dyn ErrorSink) -> bool {
        self.emit(EventKind::ArrayOpen, None, span)
    }

    fn simple_key(&mut self, span: Span, encoding: Option<Encoding>, _error: &mut dyn ErrorSink) {
        self.emit(EventKind::SimpleKey, encoding, span);
    }

    fn scalar(&mut self, span: Span, encoding: Option<Encoding>, _error: &mut dyn ErrorSink) {
        self.emit(EventKind::Scalar, encoding, span);
    }
}

/// Counts the events the admitted receiver would store, so their buffer can be
/// admitted once at its exact size. Opens are always allowed here; after a
/// refusal the stored count can only be smaller, never larger.
#[derive(Default)]
pub(super) struct Counter(pub(super) usize);

macro_rules! counted {
    ($($method:ident),* $(,)?) => {$(
        fn $method(&mut self, _span: Span, _error: &mut dyn ErrorSink) {
            self.0 = self.0.saturating_add(1);
        }
    )*};
}

impl EventReceiver for Counter {
    counted! {
        std_table_open,
        std_table_close,
        array_table_open,
        array_table_close,
        inline_table_close,
        array_close,
        key_sep,
        key_val_sep,
        value_sep,
        whitespace,
        comment,
        newline,
        error,
    }

    fn inline_table_open(&mut self, _span: Span, _error: &mut dyn ErrorSink) -> bool {
        self.0 = self.0.saturating_add(1);
        true
    }

    fn array_open(&mut self, _span: Span, _error: &mut dyn ErrorSink) -> bool {
        self.0 = self.0.saturating_add(1);
        true
    }

    fn simple_key(&mut self, _span: Span, _encoding: Option<Encoding>, _error: &mut dyn ErrorSink) {
        self.0 = self.0.saturating_add(1);
    }

    fn scalar(&mut self, _span: Span, _encoding: Option<Encoding>, _error: &mut dyn ErrorSink) {
        self.0 = self.0.saturating_add(1);
    }
}
