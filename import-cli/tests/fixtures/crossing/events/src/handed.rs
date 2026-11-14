//! What a program hands a crate in the shapes iced's tour and gallery use:
//! a label any `Into<String>` gives, as `radio(label: impl Into<String>,
//! ..)` takes; a line collected from its spans, as `Rich::from_iter<T:
//! IntoIterator<Item = Span>>(spans: T)` is; and a closure run on another
//! thread, as tokio's `spawn_blocking(f: F)` runs its `F: FnOnce() -> R +
//! Send + 'static`.

/// A crate value a closure on another thread reads.
#[derive(Debug)]
pub struct Bytes(Vec<u8>);

impl Bytes {
    /// The bytes `values`.
    #[must_use]
    pub fn new(values: &[u8]) -> Self {
        Self(values.to_vec())
    }

    /// The bytes, borrowed.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

/// The text of `label`.
pub fn labelled(label: impl Into<String>) -> String {
    format!("[{}]", label.into())
}

/// One part of a [`Line`].
#[derive(Debug)]
pub struct Span(String);

/// A span of `text`.
#[must_use]
pub fn span(text: &str) -> Span {
    Span(text.to_owned())
}

/// Spans side by side.
#[derive(Debug)]
pub struct Line(Vec<Span>);

impl Line {
    /// The text of every span.
    #[must_use]
    pub fn text(&self) -> String {
        self.0.iter().map(|span| span.0.as_str()).collect()
    }
}

impl FromIterator<Span> for Line {
    fn from_iter<T: IntoIterator<Item = Span>>(spans: T) -> Self {
        Self(spans.into_iter().collect())
    }
}

/// What `work` gives, run on a thread of its own.
pub fn off_thread<F, R>(work: F) -> R
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    std::thread::spawn(work).join().unwrap()
}
