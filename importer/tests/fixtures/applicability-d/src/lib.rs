//! Implementations whose headers hold anonymous constants, as zerocopy's
//! derives name each field by an `i128` constant, and a trait re-exported
//! from this crate's own dependency.

pub use facts_b::Assoc;

pub trait Named {}

pub struct Field<const ID: i128>;

pub struct Glyph<const C: char>;

pub struct Carrier;

const fn id(name: &str) -> i128 {
    -(name.len() as i128)
}

const fn letter(offset: u8) -> char {
    (b'a' + offset) as char
}

impl Named for Field<{ id("value") }> {}

impl Named for Field<{ id("carrier") }> {}

impl Named for Carrier where Field<{ id("carrier") }>: Named {}

impl Named for Glyph<{ letter(1) }> {}

impl Glyph<{ letter(0) }> {
    pub fn glyph() -> char {
        'a'
    }
}
