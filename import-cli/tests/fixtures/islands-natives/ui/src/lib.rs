//! A widget tree with a lifetime and a message type, as iced's `Element`,
//! and a picture's pixels, as the `image` crate's.

use std::borrow::Cow;
use std::fmt::Debug;

/// A labelled button that may send a message.
pub struct Element<'a, Message> {
    label: Cow<'a, str>,
    on_press: Option<Message>,
}

/// A button labelled `label`, which sends nothing yet.
pub fn button<'a, Message>(label: String) -> Element<'a, Message> {
    Element {
        label: Cow::Owned(label),
        on_press: None,
    }
}

impl<'a, Message: Debug> Element<'a, Message> {
    /// The button, sending `message` when pressed.
    pub fn on_press(mut self, message: Message) -> Self {
        self.on_press = Some(message);
        self
    }

    /// What the button shows and sends.
    pub fn describe(&self) -> String {
        format!("[{}] -> {:?}", self.label, self.on_press)
    }
}

/// A decoded picture, as `image::DynamicImage` is.
pub struct Picture {
    width: u32,
    height: u32,
    gray: Vec<u8>,
}

/// The picture `bytes` hold, as `image::load_from_memory` reads one: a
/// width, a height and one gray byte per pixel.
pub fn load(bytes: &[u8]) -> Result<Picture, String> {
    let [width, height, gray @ ..] = bytes else {
        return Err("no header".to_owned());
    };
    if gray.len() != usize::from(*width) * usize::from(*height) {
        return Err("short".to_owned());
    }
    Ok(Picture {
        width: u32::from(*width),
        height: u32::from(*height),
        gray: gray.to_vec(),
    })
}

impl Picture {
    /// The picture's pixels as RGBA, as `DynamicImage::to_rgba8` makes them.
    pub fn to_rgba(&self) -> Rgba {
        Rgba {
            width: self.width,
            height: self.height,
            pixels: self
                .gray
                .iter()
                .map(|&gray| Pixel([gray, gray, gray, 255]))
                .collect(),
        }
    }
}

/// One RGBA pixel, as `image::Rgba<u8>` is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pixel(pub [u8; 4]);

/// RGBA pixels by row, as `image::RgbaImage` holds them.
pub struct Rgba {
    width: u32,
    height: u32,
    pixels: Vec<Pixel>,
}

impl Rgba {
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// The pixel at `x`, `y`, to write, as `ImageBuffer::get_pixel_mut`.
    pub fn get_pixel_mut(&mut self, x: u32, y: u32) -> &mut Pixel {
        &mut self.pixels[(y * self.width + x) as usize]
    }

    /// The pixels' bytes, as `ImageBuffer::into_raw`.
    pub fn into_raw(self) -> Vec<u8> {
        self.pixels.into_iter().flat_map(|pixel| pixel.0).collect()
    }
}
