// rawr
//! A marked root with a native `icon` module, beside a `use` that binds the
//! crate's own name, `pages`, to std's `cmp`, as iced's markdown example
//! binds `markdown` to `iced::widget::markdown`. The `use` still binds
//! `pages` inside the native call's arguments.
mod icon;

use std::cmp as pages;

fn main() {
    println!("{} {}", icon::copy(), pages::max(icon::copy(), 7));
    println!("{}", icon::scaled(pages::max(3, 7)));
}
