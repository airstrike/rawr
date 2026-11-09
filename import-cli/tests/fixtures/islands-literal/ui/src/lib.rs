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

/// The sum of `values`, generic over their count, as a crate function
/// generic over `const N: usize` is (#1536).
pub fn total<const N: usize>(values: [u32; N]) -> u32 {
    values.iter().sum()
}

/// A tip built in a chain, as iced's
/// `tooltip(..).gap(..).style(container::rounded_box)` is (#1029).
pub struct Tip<'a> {
    size: u32,
    gap: u32,
    style: Box<dyn Fn(u32) -> u32 + 'a>,
}

/// A tip of `size`, in no style.
pub fn tip<'a>(size: u32) -> Tip<'a> {
    Tip {
        size,
        gap: 0,
        style: Box::new(|size| size),
    }
}

impl<'a> Tip<'a> {
    /// The tip `gap` away from its content.
    pub fn gap(mut self, gap: u32) -> Self {
        self.gap = gap;
        self
    }

    /// The tip in `style`.
    pub fn style(mut self, style: impl Fn(u32) -> u32 + 'a) -> Self {
        self.style = Box::new(style);
        self
    }

    pub fn size(&self) -> u32 {
        (self.style)(self.size) + self.gap
    }
}
