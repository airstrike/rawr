// Native code of the program's own, as iced's download_progress has its
// `download.rs`. The tests copy it to `src/`, and only `stepped.rs`,
// `sipped.rs` and `finished.rs` declare it, each using one function.
#![allow(dead_code)]

/// Steps of `url`'s length, behind an opaque type that captures the `impl
/// AsRef<str>` input, as `download(url: impl AsRef<str>) -> impl Straw<..>`
/// does under edition 2024.
pub fn download(url: impl AsRef<str>) -> impl events::Steps {
    let length = url.as_ref().len() as u32;
    (1..=3).map(move |step| step * length)
}

/// Sizes of `url`'s length, reported as the sipper's own `Item`.
pub fn sizes(url: impl AsRef<str>) -> impl events::Sip<Item = u32> {
    Sizes(url.as_ref().len() as u32)
}

struct Sizes(u32);

impl events::Sip for Sizes {
    type Item = u32;

    fn sipped(self) -> Vec<u32> {
        vec![self.0, self.0 * 2]
    }
}

/// An image the gallery lists, as iced's gallery's `civitai::Image`.
#[derive(Debug, Clone)]
pub struct Image {
    pub id: u32,
    pub url: String,
}

impl Image {
    /// As many placeholders as the gallery shows before it has images.
    pub const LIMIT: usize = 2;

    /// The image's bytes, or an error, behind `impl Straw<..>`, as iced's
    /// gallery's `Image::download(self, size)` gives.
    pub fn download(self, size: Size) -> impl events::Straw<Bytes, u32, String> {
        Downloaded(match size {
            Size::Original => self.url.len() as u32 + self.id,
            Size::Thumbnail { width } => width,
        })
    }
}

/// Downloaded bytes, as their count, as iced's gallery's `civitai::Bytes`.
#[derive(Debug, Clone)]
pub struct Bytes(pub u32);

struct Downloaded(u32);

impl Future for Downloaded {
    type Output = Result<Bytes, String>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        std::task::Poll::Ready(match self.0 {
            0 => Err("empty".to_owned()),
            length => Ok(Bytes(length * 3)),
        })
    }
}

impl events::Sip for Downloaded {
    type Item = u32;

    fn sipped(self) -> Vec<u32> {
        vec![self.0]
    }
}

/// What the gallery downloads, as iced's gallery's `civitai::Size`.
#[derive(Debug, Clone, Copy)]
pub enum Size {
    Original,
    Thumbnail { width: u32 },
}

impl std::fmt::Display for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}#{}", self.url, self.id)
    }
}

/// The length of `url` as the download's progress, and three times it or
/// an error as its output, behind `impl Straw<..>`, as iced's
/// download_progress's `download` gives.
pub fn finished(url: impl AsRef<str>) -> impl events::Straw<u32, u32, String> {
    Finished(url.as_ref().len() as u32)
}

struct Finished(u32);

impl Future for Finished {
    type Output = Result<u32, String>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        std::task::Poll::Ready(match self.0 {
            0 => Err("empty".to_owned()),
            length => Ok(length * 3),
        })
    }
}

impl events::Sip for Finished {
    type Item = u32;

    fn sipped(self) -> Vec<u32> {
        vec![self.0]
    }
}

/// `value` alone in a `Vec`, as a headless run of iced's download_progress
/// collects a task's outputs: a generic function the program's own type
/// fills.
pub fn wrapped<T>(value: T) -> Vec<T> {
    vec![value]
}
