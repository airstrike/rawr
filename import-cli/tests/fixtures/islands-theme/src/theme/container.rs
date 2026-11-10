//! Native: a style function of the marked `theme`'s, as inboard's
//! `theme/container.rs`.
use crate::theme::Theme;

pub fn base(theme: &Theme) -> u32 {
    theme.accent() + 1
}

/// The padding a read inbox item has, as inboard's `inbox_item` gives a
/// container's style.
pub fn item(theme: &Theme) -> ui::Padding {
    ui::Padding {
        vertical: 1.0,
        horizontal: theme.accent() as f32,
    }
}

/// The padding an unread inbox item has, as inboard's `inbox_item_unread`.
pub fn item_unread(theme: &Theme) -> ui::Padding {
    ui::Padding {
        vertical: 2.0,
        horizontal: theme.accent() as f32 + 0.5,
    }
}
