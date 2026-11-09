//! Native: a crate trait implemented for the marked `Theme` by matching
//! its variant, as inboard's `theme/markdown.rs` does.
use crate::theme::Theme;

impl ui::Catalog for Theme {
    fn id(&self) -> &str {
        match self {
            Theme::Selected(theme) => &theme.name,
        }
    }
}
