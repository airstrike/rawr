//! A stand-in for iced's `Text::style`, which takes a function of the theme.

/// `size` in `style`.
pub fn styled(size: u32, style: impl Fn(u32) -> u32) -> u32 {
    style(size)
}

/// A caption whose `style` method takes a function, as iced's
/// `Container::style` does.
pub struct Caption {
    size: u32,
}

impl Caption {
    pub fn new(size: u32) -> Self {
        Self { size }
    }

    /// The caption in `style`.
    pub fn style(self, style: impl Fn(u32) -> u32) -> Self {
        Self {
            size: style(self.size),
        }
    }

    pub fn size(&self) -> u32 {
        self.size
    }
}
