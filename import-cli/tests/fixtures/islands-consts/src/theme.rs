// rawr
//! The app's theme, marked, as inboard's `theme.rs`: it names its native
//! workspace crate's `&'static str` and slice constants through the crate's
//! module, and gives them to the crate's `Font::new(&'static str)`.
use common::fonts;

pub fn font() -> fonts::Font {
    fonts::Font::new(fonts::UI_FAMILY)
}

pub fn code() -> fonts::Font {
    fonts::Font::new(fonts::CODE_FAMILY)
}

pub fn bytes() -> usize {
    fonts::BYTES.len()
}

pub fn weights() -> Vec<u16> {
    let mut weights = Vec::new();
    for weight in fonts::WEIGHTS {
        weights.push(*weight);
    }
    weights
}

pub fn family() -> String {
    let family: &'static str = fonts::UI_FAMILY;
    format!("{family} has {} letters", family.len())
}

pub fn version() -> String {
    format!("version {}", version::VALUE)
}

pub fn heading() -> String {
    let heading: &'static fonts::Font = &fonts::HEADING;
    format!(
        "heading {} {}",
        fonts::HEADING.family(),
        fonts::family_of(heading)
    )
}
