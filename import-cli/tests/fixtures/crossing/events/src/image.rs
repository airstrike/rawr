//! Images in the shape of iced's: a handle a program keeps, and an image
//! made from a borrow of one, as iced's `canvas::Image::new(&handle)` and
//! `frame.draw_image(bounds, &handle)` take one through `impl Into<_>`.

/// What an image shows, as iced's `image::Handle`.
#[derive(Debug, Clone, PartialEq)]
pub struct Handle {
    name: String,
}

impl Handle {
    /// The image named `name`.
    pub fn from_name(name: &str) -> Self {
        Self {
            name: name.to_owned(),
        }
    }
}

/// A borrowed handle is a handle, as iced's `impl From<&Handle> for Handle`.
impl From<&Handle> for Handle {
    fn from(handle: &Handle) -> Self {
        handle.clone()
    }
}

/// A handle drawn turned, as iced's `canvas::Image`.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    handle: Handle,
    rotation: f32,
}

impl Image {
    /// `handle` drawn upright.
    pub fn new(handle: impl Into<Handle>) -> Self {
        Self {
            handle: handle.into(),
            rotation: 0.0,
        }
    }

    /// The image turned by `rotation`.
    #[must_use]
    pub fn rotation(mut self, rotation: f32) -> Self {
        self.rotation = rotation;
        self
    }
}

/// A borrowed handle is an image of it, as iced's `impl From<&Handle> for
/// Image`.
impl From<&Handle> for Image {
    fn from(handle: &Handle) -> Self {
        Self::new(handle)
    }
}

/// What drawing `image` shows, as iced's `Frame::draw_image` takes `impl
/// Into<Image>`.
pub fn draw(image: impl Into<Image>) -> String {
    let image = image.into();
    format!("{} turned {}", image.handle.name, image.rotation)
}
