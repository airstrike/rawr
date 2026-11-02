pub struct Lanes<const N: u32>;

pub struct Switch<const ON: bool>;

pub struct Glyph<const C: char>;

pub fn lanes() -> Lanes<{ 2 + 2 }> {
    Lanes
}

pub fn switch() -> Switch<{ !false }> {
    Switch
}

pub fn glyph() -> Glyph<{ char::MAX }> {
    Glyph
}

pub fn plain() -> u8 {
    1
}
