//! Native: a button style function of the marked `theme`'s, as inboard's
//! `theme/button.rs`.
#![allow(dead_code)]
use ui::button::{Status, Style};

use crate::theme::Theme;

/// A style for a selected or plain row, as inboard's `sidebar_nav`: a
/// closure a caller hands on to a button's `style`.
pub fn sidebar_nav<'a>(selected: bool) -> impl Fn(&Theme, Status) -> Style + 'a {
    move |theme, status| Style {
        text_color: u32::from(selected) + theme.colors().text,
        ..board_toolbar(theme, status)
    }
}

pub fn board_toolbar(theme: &Theme, status: Status) -> Style {
    let active = Style {
        text_color: theme.colors().text,
        background: theme.accent(),
    };
    match status {
        Status::Active => active,
        Status::Hovered => Style {
            background: theme.accent() + 2,
            ..active
        },
    }
}
