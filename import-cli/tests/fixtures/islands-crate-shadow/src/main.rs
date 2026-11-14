// rawr
//! A marked root with a native `icon` module, beside a `use` that binds the
//! crate's own name, `pages`, to std's `cmp`, as iced's markdown example
//! binds `markdown` to `iced::widget::markdown`.
mod icon;

use std::cmp as pages;

fn main() {
    println!("{} {}", icon::copy(), pages::max(icon::copy(), 7));
}
