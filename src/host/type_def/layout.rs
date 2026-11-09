//! rustc's layout of an imported struct, as far as a cycle of statics of
//! it needs (D865): where its pointers that may hold a static's address
//! keep their address words.

/// Where a struct keeps the address words of its pointers that may hold
/// a static's address, as rustc lays it out.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Layout {
    size: u64,
    pointers: Box<[u64]>,
    varied: bool,
}

impl Layout {
    /// A struct of `size` bytes with pointer words at `pointers`, outside
    /// any enum variant or union; `varied` when another such pointer is
    /// inside one.
    #[cfg(feature = "rust-import")]
    pub(crate) const fn new(size: u64, pointers: Box<[u64]>, varied: bool) -> Self {
        Self {
            size,
            pointers,
            varied,
        }
    }

    /// The struct's size in bytes.
    #[must_use]
    pub const fn size(&self) -> u64 {
        self.size
    }

    /// The offsets of its pointer words outside any enum variant or union.
    #[must_use]
    pub fn pointers(&self) -> &[u64] {
        &self.pointers
    }

    /// Whether a pointer that may hold a static's address is inside an enum
    /// variant or a union, where its word is there only in some values.
    #[must_use]
    pub const fn varied(&self) -> bool {
        self.varied
    }
}
