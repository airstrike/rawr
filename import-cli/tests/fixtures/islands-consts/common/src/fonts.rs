//! The app's fonts, as inboard's `common::fonts`.

/// The family the interface is drawn in.
pub const UI_FAMILY: &str = "Inter Tight";

/// The bytes of the bundled font file.
pub const BYTES: &[u8] = b"\x00\x01\x00\x00font";

/// The weights the bundled family has.
pub const WEIGHTS: &[u16] = &[400, 500, 700];

/// The family code is drawn in, a `static`.
pub static CODE_FAMILY: &str = "Iosevka";

/// A font of a family, as `iced::Font`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Font {
    family: &'static str,
}

impl Font {
    pub fn new(name: &'static str) -> Self {
        Self { family: name }
    }

    pub fn family(&self) -> &'static str {
        self.family
    }
}

/// The heading font, made on first use, as loading_spinners' easings are.
pub static HEADING: std::sync::LazyLock<Font> =
    std::sync::LazyLock::new(|| Font::new("Inter Display"));

/// The family of a font lent for good.
pub fn family_of(font: &'static Font) -> &'static str {
    font.family
}
