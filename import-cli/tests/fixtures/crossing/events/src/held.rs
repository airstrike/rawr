//! A crate's concrete holder of any `Shown`, in the shape of iced's
//! `Element`: its generic constructor boxes the item as the trait's
//! object, and its concrete code calls it through that object.

use crate::framed::{Canvas, Drawn, Meta, Shown};

/// An item held as its trait object.
pub struct Held<'a>(Box<dyn Shown + 'a>);

impl<'a> Held<'a> {
    /// `item`, held as its trait object.
    pub fn new(item: impl Shown + 'a) -> Self {
        Self(Box::new(item))
    }

    /// The item's text, in parentheses.
    #[must_use]
    pub fn shown(&self) -> String {
        format!("({})", self.0.shown())
    }
}

/// A swatch drawn onto any canvas its impls name, as iced's `Text<'a,
/// Theme, Renderer>` is a widget at the theme its impl names: only the
/// impl for the crate's `Canvas` draws one, so a frame's impl fixes it.
pub struct Swatch<C>(String, std::marker::PhantomData<C>);

/// A swatch named `name`, at a canvas its use fixes.
pub fn swatch<C>(name: &str) -> Swatch<C> {
    Swatch(name.to_owned(), std::marker::PhantomData)
}

impl<C> Meta for Swatch<C> {}

impl<C> Shown for Swatch<C> {
    fn shown(&self) -> String {
        self.0.clone()
    }
}

impl Drawn<Canvas> for Swatch<Canvas> {
    fn draw(&self, canvas: &mut Canvas) {
        canvas.mark("%");
        canvas.mark(&self.0);
    }
}

/// A cell held as its trait object at the canvas `C`, as iced's `Element`.
pub struct Drawing<'a, C>(pub(crate) Box<dyn Drawn<C> + 'a>);

/// Each of `items` drawn onto one canvas, by the crate's concrete code.
#[must_use]
pub fn drawings(items: &[Drawing<'_, Canvas>]) -> String {
    let mut canvas = Canvas::default();
    for item in items {
        item.0.draw(&mut canvas);
        canvas.mark(" ");
    }
    canvas.marks().to_owned()
}
