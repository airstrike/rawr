//! Presentation-independent structured diagnostics.

mod adapter;
pub mod code;
mod render;
mod report;
mod source;
mod view;

pub use render::render;
pub use crate::types::Rendered;
pub use report::{
    Builder, Category, Error, ExternalLabel, ExternalPosition, Label, LabelKind, Report, Severity,
};
pub use source::Source;
pub use view::{ExternalLabelView, LabelView, PositionView, ReportView, SourceView, SpanView};
