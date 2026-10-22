// Pinned toml_parser 1.1.2+spec-1.1.0; mapping and fixed cfgs: toml/provenance.json.
//! A TOML push [parser][parse_document]
//!
//! This takes TOML [tokens][crate::lexer::Token] and [emits][EventReceiver] [events][Event].

mod document;
mod event;

pub use document::parse_document;
pub use event::Event;
pub use event::EventKind;
pub use event::EventReceiver;
pub use event::RecursionGuard;
pub use event::ValidateWhitespace;
