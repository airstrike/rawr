//! Native: a mouse area style function of the marked `theme`'s, as
//! inboard's `theme/mouse_area.rs`, whose last argument a caller gives
//! ahead with `common::function::Ternary::with`.
#![allow(dead_code)]
use ui::mouse_area::Status;

use crate::theme::Theme;

pub fn muted_tab(theme: &Theme, status: Status, selected: bool) -> u32 {
    match (status, selected) {
        (_, true) => theme.accent(),
        (Status::Hovered, false) => theme.colors().text,
        (Status::Idle, false) => 0,
    }
}
