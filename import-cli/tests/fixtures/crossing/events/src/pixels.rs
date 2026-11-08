//! Pixels in the shape of the image crate's: a tuple struct with a public
//! array field, and a buffer whose methods, inherent and of a trait, lend
//! one of its pixels `&mut`, as `GenericImage::get_pixel_mut` does.

/// A pixel's four channels, as image's `Rgba<T>(pub [T; 4])`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pixel(pub [u8; 4]);

/// Pixels in a row, as image's `ImageBuffer`.
#[derive(Debug, Default)]
pub struct Buffer {
    pixels: Vec<Pixel>,
    /// How often a pixel was lent mutably.
    lent: usize,
}

impl Buffer {
    /// `count` transparent pixels.
    pub fn new(count: usize) -> Self {
        Self {
            pixels: vec![Pixel([0; 4]); count],
            lent: 0,
        }
    }

    /// The pixel at `index`, lent mutably.
    pub fn get_mut(&mut self, index: usize) -> &mut Pixel {
        self.lent += 1;
        &mut self.pixels[index]
    }

    /// How often `get_mut` and `pixel_mut` were called.
    pub fn lent(&self) -> usize {
        self.lent
    }

    /// The pixel at `index`.
    pub fn get(&self, index: usize) -> Pixel {
        self.pixels[index]
    }

    /// Every pixel's channels, as the crate reads them.
    pub fn describe(&self) -> String {
        format!("{:?}", self.pixels)
    }
}

/// An image whose pixels can be changed in place, as image's
/// `GenericImage`.
pub trait Image {
    /// The pixel at `index`, lent mutably.
    fn pixel_mut(&mut self, index: usize) -> &mut Pixel;
}

impl Image for Buffer {
    fn pixel_mut(&mut self, index: usize) -> &mut Pixel {
        self.lent += 1;
        &mut self.pixels[index]
    }
}

/// Sets every channel of `pixel` to `value`.
pub fn fill(pixel: &mut Pixel, value: u8) {
    pixel.0 = [value; 4];
}
