//! Presentation-independent structured diagnostics.

mod adapter;
pub mod code;
mod panic;
mod render;
mod report;
pub(crate) mod source;
mod view;

pub use crate::types::Rendered;
#[cfg(any(feature = "sync", feature = "rust-import"))]
pub(crate) use panic::located;
pub use panic::{locate_panics_with, panic, panic_message, panicked, BACKTRACE_NOTE};
pub use render::render;
pub use report::{
    Builder, Category, Error, ExternalLabel, ExternalPosition, Label, LabelKind, Report, Severity,
};
pub use source::Source;
pub use view::{ExternalLabelView, LabelView, PositionView, ReportView, SourceView, SpanView};
