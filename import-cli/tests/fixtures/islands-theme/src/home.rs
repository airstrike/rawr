// rawr
//! A marked screen, as inboard's `home.rs`: it imports the marked `theme`
//! and `widget`'s `text`, and names the theme's native child modules
//! `theme::container` and `theme::text` through the imported `theme`. Each
//! style function is a crate widget's `style`, and each calls its own.
use crate::theme;
use crate::widget::text;

pub fn heading() -> String {
    format!("{} at {}", text("home"), theme::text::SIZE)
}

pub fn container() -> ui::Label<'static, theme::Theme> {
    ui::label("container").style(theme::container::base)
}

pub fn label() -> ui::Label<'static, theme::Theme> {
    ui::label("text").style(theme::text::base)
}
