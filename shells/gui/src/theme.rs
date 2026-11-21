//! How the window's panes are drawn, dense as DevTools' panes: one text
//! size for every row, bars over the panes, the split lines between them,
//! and the colours a value, a selected row and the marked line take. Each
//! colour comes from the window's theme, so light and dark both hold. The
//! window is dark in Oxocarbon and light in iced's light theme.

mod colour;
mod widget;

pub use colour::*;
pub use widget::*;

/// The size of every row's text.
pub const SIZE: f32 = 12.0;

/// The size of a bar's caption and what a bar says.
pub const CAPTION: f32 = 11.0;
