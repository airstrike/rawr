//! Presentation-independent structured diagnostics.

mod adapter;
mod arguments;
mod arm;
pub mod code;
mod definition;
mod implemented;
mod panic;
mod render;
mod report;
mod requirement;
pub mod section;
pub(crate) mod source;
pub mod suggestion;
mod view;
pub mod wire;

pub use crate::types::Rendered;
pub(crate) use panic::located;
pub use panic::{hooked, locate_panics_with, note, panic, panic_message, panicked, BACKTRACE_NOTE};
pub(crate) use render::offset;
pub use render::{render, render_with, show_paths_from, Color};
pub use report::{
    Afterword, Builder, Category, Error, ExternalLabel, ExternalPosition, Label, LabelKind, Report,
    Severity,
};
pub use section::Section;
pub use source::Source;
pub use suggestion::Suggestion;
pub use view::{
    EditView, ExternalLabelView, LabelView, PositionView, ReportView, SectionView, SourceView,
    SpanView, SuggestionView,
};
