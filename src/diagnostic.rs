//! Presentation-independent structured diagnostics.

mod adapter;
pub mod code;
mod panic;
mod render;
mod report;
mod source;
mod view;

pub use crate::types::Rendered;
pub use panic::{panic, BACKTRACE_NOTE};
pub use render::render;
pub use report::{
    Builder, Category, Error, ExternalLabel, ExternalPosition, Label, LabelKind, Report, Severity,
};
pub use source::Source;
pub use view::{ExternalLabelView, LabelView, PositionView, ReportView, SourceView, SpanView};
