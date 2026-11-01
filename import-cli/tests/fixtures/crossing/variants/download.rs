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
}

impl std::fmt::Display for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}#{}", self.url, self.id)
    }
}

/// The length of `url`, once the download is done, as a future.
pub fn finished(url: impl AsRef<str>) -> impl Future<Output = u32> {
    let length = url.as_ref().len() as u32;
    async move { length }
}
