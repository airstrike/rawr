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

/// An image at a path, as iced's `impl<T: Into<PathBuf>> From<T> for
/// Handle` makes one of a `&str`.
impl From<&str> for Handle {
    fn from(path: &str) -> Self {
        Self::from_name(path)
    }
}

/// Bytes an image is made of, as iced's `Bytes`, which a `&'static [u8]`
/// converts into.
pub struct Bytes(&'static [u8]);

impl From<&'static [u8]> for Bytes {
    fn from(bytes: &'static [u8]) -> Self {
        Self(bytes)
    }
}

impl Handle {
    /// The image of `bytes`, named by their count, as iced's
    /// `Handle::from_bytes(impl Into<Bytes>)`.
    pub fn from_bytes(bytes: impl Into<Bytes>) -> Self {
        Self {
            name: format!("{} bytes", bytes.into().0.len()),
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
