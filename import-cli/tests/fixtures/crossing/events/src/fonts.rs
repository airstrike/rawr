//! Fonts in the shape of fount's `system` module: a crate struct, an error
//! of the crate's own, and an `async fn` that borrows the struct.

/// A font found on disk, as fount's `system::Font`.
#[derive(Debug, Clone)]
pub struct Font {
    pub family: String,
}

/// The crate's error, as fount's `Error`.
#[derive(Debug)]
pub struct Error(String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "no font named {:?}", self.0)
    }
}

/// The font's bytes, as fount's `system::load(&Font)` reads its file.
pub async fn load(font: &Font) -> Result<Vec<u8>, Error> {
    if font.family.is_empty() {
        Err(Error(font.family.clone()))
    } else {
        Ok(font.family.bytes().collect())
    }
}
