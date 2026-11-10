//! Functions and methods whose type parameter appears only inside an
//! argument's bound, as iced's `rich_text(impl AsRef<[Span<'a, Link>]>)`:
//! a call takes the parameter from std's or the crate's impl of the bound
//! for the argument's type.

use std::borrow::Borrow;

/// A piece of text with an optional link, as iced's `text::Span`.
#[derive(Debug, Clone)]
pub struct Span<'a, Link = ()> {
    pub text: &'a str,
    pub link: Option<Link>,
}

/// A span of `text` without a link.
pub fn span<'a, Link>(text: &'a str) -> Span<'a, Link> {
    Span { text, link: None }
}

/// The spans a `Rich` shows, as iced's `text::Rich`.
pub struct Rich<'a, Link> {
    spans: Vec<Span<'a, Link>>,
    size: u16,
}

impl<'a, Link: Clone> Rich<'a, Link> {
    /// The text of `spans` in `size`, as iced's `rich_text(..).size(..)`.
    pub fn size(mut self, size: u16) -> Self {
        self.size = size;
        self
    }

    /// Each span's text, and the size.
    pub fn shown(&self) -> String {
        let texts = self
            .spans
            .iter()
            .map(|span| span.text)
            .collect::<Vec<_>>()
            .join("|");
        format!("rich {texts} at {}", self.size)
    }

    /// `links` beside these spans' texts, a method's own parameter that
    /// only its argument's bound names.
    pub fn beside<M: std::fmt::Debug>(&self, links: impl AsRef<[M]>) -> String {
        format!("{} beside {:?}", self.spans.len(), links.as_ref())
    }

    /// The spans of `more` after these, through `IntoIterator`.
    pub fn extended(mut self, more: impl IntoIterator<Item = Span<'a, Link>>) -> Self {
        self.spans.extend(more);
        self
    }
}

/// The rich text of `spans`, as iced's `rich_text`.
pub fn rich<'a, L: Clone>(spans: impl AsRef<[Span<'a, L>]> + 'a) -> Rich<'a, L> {
    Rich {
        spans: spans.as_ref().to_vec(),
        size: 16,
    }
}

/// The rich text of `spans`, borrowed through `Borrow`.
pub fn borrowed<'a, L: Clone, S: Borrow<[Span<'a, L>]>>(spans: S) -> Rich<'a, L> {
    Rich {
        spans: spans.borrow().to_vec(),
        size: 12,
    }
}

/// The rich text of `spans`, through `IntoIterator`.
pub fn gathered<'a, L>(spans: impl IntoIterator<Item = Span<'a, L>>) -> Rich<'a, L> {
    Rich {
        spans: spans.into_iter().collect(),
        size: 14,
    }
}

/// A crate's own trait whose parameter a crate impl fixes.
pub trait Linked<L> {
    fn first(&self) -> Option<L>;
}

/// Links of `u8`s.
pub struct Numbers(pub Vec<u8>);

impl Linked<u8> for Numbers {
    fn first(&self) -> Option<u8> {
        self.0.first().copied()
    }
}

/// The first link of `linked`, generic over what it links.
pub fn first_link<L: std::fmt::Debug>(linked: impl Linked<L>) -> String {
    format!("first {:?}", linked.first())
}
