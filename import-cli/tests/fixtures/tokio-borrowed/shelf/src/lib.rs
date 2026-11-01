//! Async functions that borrow their arguments, as fount's
//! `async fn load(font: &Font) -> Result<Vec<u8>, Error>`: each future
//! holds its borrow across an await.

/// A font file, as fount's `system::Font`.
#[derive(Debug, Clone)]
pub struct Font {
    /// Its family's name.
    pub family: String,
    /// Its bytes.
    pub bytes: Vec<u8>,
}

/// A font of `family` with `size` bytes.
pub fn font(family: String, size: u8) -> Font {
    Font {
        family,
        bytes: (0..size).collect(),
    }
}

/// The font's size, read after a yield, through the borrow its future
/// holds.
pub async fn load(font: &Font) -> usize {
    tokio::task::yield_now().await;
    font.bytes.len()
}

/// The font renamed after a yield, through the mutable borrow its future
/// holds.
pub async fn rename(font: &mut Font, family: String) {
    tokio::task::yield_now().await;
    font.family = family;
}
