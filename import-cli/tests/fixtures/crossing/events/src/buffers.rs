//! Buffers a call writes into, as pulldown-cmark's `push_html(&mut
//! String, ..)` and quick-xml's `read_event_into(&mut Vec<u8>)` take
//! them, and a trait a string is, as iced's `impl Widget for &str`.

/// Room for `additional` more bytes in `out`.
pub fn reserve(out: &mut String, additional: usize) {
    out.reserve(additional);
}

/// `line` and a newline, written at the end of `out`.
pub fn write_line(out: &mut String, line: &str) {
    out.push_str(line);
    out.push('\n');
}

/// How many bytes `out` holds room for: the same allocation keeps its
/// capacity from one call to the next.
pub fn room(out: &mut String) -> usize {
    out.capacity()
}

/// Room for `additional` more bytes in `buffer`.
pub fn reserve_bytes(buffer: &mut Vec<u8>, additional: usize) {
    buffer.reserve(additional);
}

/// `count` copies of `byte` at the end of `buffer`.
pub fn fill(buffer: &mut Vec<u8>, byte: u8, count: usize) {
    buffer.extend(std::iter::repeat_n(byte, count));
}

/// How many bytes `buffer` holds room for.
pub fn byte_room(buffer: &mut Vec<u8>) -> usize {
    buffer.capacity()
}

/// A string that is a label, as iced's `impl Widget for &str` makes one
/// of a string literal: the impl holds for a `&str` of any lifetime.
pub trait Label {
    /// The label's width, which borrows nothing past the call.
    fn width(self) -> usize;

    /// The label, shown, which borrows the string for as long as it lives.
    fn shown<'a>(self) -> Shown<'a>
    where
        Self: 'a;
}

/// A label that borrows its text.
pub struct Shown<'a> {
    text: &'a str,
}

impl Shown<'_> {
    /// The text it shows.
    pub fn text(&self) -> String {
        format!("[{}]", self.text)
    }
}

impl Label for &str {
    fn width(self) -> usize {
        self.chars().count()
    }

    fn shown<'a>(self) -> Shown<'a>
    where
        Self: 'a,
    {
        Shown { text: self }
    }
}
