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

/// What `f` makes of the output of `future`, polled to its end on a thread
/// of its own, as iced's `Task::perform(future, f)` maps the output of a
/// future a tokio worker polls.
pub fn perform<T, A>(
    future: impl Future<Output = T> + Send + 'static,
    f: impl FnOnce(T) -> A + Send + 'static,
) -> A
where
    T: Send + 'static,
    A: Send + 'static,
{
    std::thread::spawn(move || f(block_on(future)))
        .join()
        .unwrap()
}

/// The output of `future`, polled on this thread till it is ready.
fn block_on<T>(future: impl Future<Output = T>) -> T {
    use std::task::{Context, Poll, Waker};
    let mut future = std::pin::pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
        std::thread::yield_now();
    }
}
