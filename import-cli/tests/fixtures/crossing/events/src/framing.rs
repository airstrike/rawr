//! A trait whose method gives an opaque type, in the shape of iced
//! markdown's `Viewer::code_block(..) -> impl Widget<Message, Theme,
//! Renderer> + 'a` (#2013): a program's impl gives a crate type that holds
//! another crate function's opaque output, which no Rust signature names.
//! The trait has a type argument with a default and a supertrait, and a
//! method that borrows its receiver for what it gives.

/// What every shape can say of itself.
pub trait Named {
    fn name(&self) -> String;
}

/// A shape measured in `T`.
pub trait Shape<T>: Named {
    /// Its area.
    fn area(&self) -> T;

    /// Grows each side by `by`.
    fn grow(&mut self, by: T);

    /// The names of its parts, borrowed from the shape.
    fn parts<'a>(&'a self) -> Vec<&'a str>;
}

struct Square {
    side: u32,
}

impl Named for Square {
    fn name(&self) -> String {
        "square".to_owned()
    }
}

impl Shape<u32> for Square {
    fn area(&self) -> u32 {
        self.side * self.side
    }

    fn grow(&mut self, by: u32) {
        self.side += by;
    }

    fn parts<'a>(&'a self) -> Vec<&'a str> {
        vec!["side"]
    }
}

/// A square of `side`, as an opaque shape.
pub fn square(side: u32) -> impl Shape<u32> {
    Square { side }
}

/// A label a frame shows, borrowed, as iced's `Container<'a, ..>` borrows
/// what it holds.
pub struct Caption<'a> {
    text: &'a str,
}

/// A caption of `text`.
pub fn caption(text: &str) -> Caption<'_> {
    Caption { text }
}

/// A shape inside a frame with a caption.
pub struct Framed<'a, S> {
    inner: S,
    caption: Caption<'a>,
}

/// `inner` framed with `caption`, as iced's `hover(base, top)` holds both.
pub fn framed<S>(inner: S, caption: Caption<'_>) -> Framed<'_, S> {
    Framed { inner, caption }
}

impl<S: Named> Named for Framed<'_, S> {
    fn name(&self) -> String {
        format!("{} framed", self.inner.name())
    }
}

impl<S: Shape<u32>> Shape<u32> for Framed<'_, S> {
    fn area(&self) -> u32 {
        self.inner.area() + 1
    }

    fn grow(&mut self, by: u32) {
        self.inner.grow(by);
    }

    fn parts<'a>(&'a self) -> Vec<&'a str> {
        let mut parts = self.inner.parts();
        parts.push(self.caption.text);
        parts
    }
}

/// What makes the shapes a gallery shows.
pub trait Gallery<'a, T = u32> {
    /// The shape of `side`.
    fn shape(&self, side: T) -> impl Shape<T> + 'a;
}

/// The gallery's shape of `side`, grown by one, said.
pub fn show<'a>(gallery: &impl Gallery<'a>, side: u32) -> String {
    let mut shape = gallery.shape(side);
    shape.grow(1);
    format!("{} {} {:?}", shape.name(), shape.area(), shape.parts())
}
