//! Makes the window's icon module, src/icon.rs, and the font it draws
//! with, fonts/lucide.ttf, from the icons fonts/icons.toml names.

fn main() {
    println!("cargo::rerun-if-changed=fonts/icons.toml");
    iced_lucide::build("fonts/icons.toml").expect("the icons in fonts/icons.toml are Lucide's");
}
