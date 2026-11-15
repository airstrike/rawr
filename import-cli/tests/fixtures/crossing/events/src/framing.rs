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

    /// The shape labelled `label`, which it borrows for what it gives, as
    /// iced markdown's `Viewer::code_block(.., code: &'a str, ..)` is.
    fn outline(&self, label: &'a str) -> impl Shape<T> + 'a;

    /// The shape of `parts`, which it borrows for what it gives, as iced
    /// markdown's `Viewer::code_block(.., lines: &'a [Text])` is.
    fn lines(&self, parts: &'a [Part]) -> impl Shape<T> + 'a;
}

/// A part of a plan.
#[derive(Clone, Debug)]
pub struct Part {
    name: String,
}

/// A plan of parts, as iced markdown's `Content` holds its items.
pub struct Plan {
    parts: Vec<Part>,
}

impl Plan {
    /// A plan of a part for each of `names`.
    pub fn new(names: Vec<String>) -> Self {
        Self {
            parts: names.into_iter().map(|name| Part { name }).collect(),
        }
    }

    /// Its parts, lent, as `Content::items(&self) -> &[Item]` lends them.
    pub fn parts(&self) -> &[Part] {
        &self.parts
    }
}

/// A shape traced along a borrowed label.
struct Traced<'a> {
    label: &'a str,
}

impl Named for Traced<'_> {
    fn name(&self) -> String {
        "trace".to_owned()
    }
}

impl Shape<u32> for Traced<'_> {
    fn area(&self) -> u32 {
        self.label.len() as u32
    }

    fn grow(&mut self, _by: u32) {}

    fn parts<'b>(&'b self) -> Vec<&'b str> {
        vec![self.label]
    }
}

/// A shape traced along `label`, which it borrows, as iced markdown's
/// `code_block(viewer, settings, lines, on_link)` borrows its inputs.
pub fn traced(label: &str) -> impl Shape<u32> + '_ {
    Traced { label }
}

/// A shape lined with the names of borrowed parts, each tagged.
struct Lined<'a, F> {
    parts: &'a [Part],
    tag: F,
    base: u32,
}

impl<F: Fn(String) -> String> Named for Lined<'_, F> {
    fn name(&self) -> String {
        let tags = self
            .parts
            .iter()
            .map(|part| (self.tag)(part.name.clone()))
            .collect::<Vec<_>>();
        format!("lined {}", tags.join(" "))
    }
}

impl<F: Fn(String) -> String> Shape<u32> for Lined<'_, F> {
    fn area(&self) -> u32 {
        self.base + self.parts.len() as u32
    }

    fn grow(&mut self, _by: u32) {}

    fn parts<'b>(&'b self) -> Vec<&'b str> {
        self.parts.iter().map(|part| part.name.as_str()).collect()
    }
}

/// A shape of `gallery` lined with `parts`, which it keeps borrowed, each
/// tagged by `tag`, as iced markdown's `code_block(viewer, settings, lines,
/// on_link_click)` keeps `lines` and takes a function.
pub fn lined<'a>(
    gallery: &impl Gallery<'a>,
    parts: &'a [Part],
    tag: impl Fn(String) -> String + Clone + 'a,
) -> impl Shape<u32> + 'a {
    let base = gallery.shape(1).area();
    Lined { parts, tag, base }
}

/// What a gallery shows of a plan, borrowed from the plan's parts, as
/// iced markdown's `view_with(items, settings, viewer)` gives an
/// `Element<'a, ..>`.
pub struct Listing<'a> {
    parts: &'a [Part],
    shape: Box<dyn Shape<u32> + 'a>,
    lines: Box<dyn Shape<u32> + 'a>,
}

impl Listing<'_> {
    /// What it shows.
    pub fn render(&self) -> String {
        let names = self
            .parts
            .iter()
            .map(|part| part.name.as_str())
            .collect::<Vec<_>>();
        format!(
            "{names:?}: {} {} {:?}; {} {} {:?}",
            self.shape.name(),
            self.shape.area(),
            self.shape.parts(),
            self.lines.name(),
            self.lines.area(),
            self.lines.parts()
        )
    }
}

/// The gallery's listing of `parts`, which it keeps borrowed.
pub fn listing<'a>(parts: &'a [Part], gallery: &impl Gallery<'a>) -> Listing<'a> {
    Listing {
        parts,
        shape: Box::new(gallery.outline("plan")),
        lines: Box::new(gallery.lines(parts)),
    }
}

/// The gallery's shape of `side`, grown by one, said.
pub fn show<'a>(gallery: &impl Gallery<'a>, side: u32) -> String {
    let mut shape = gallery.shape(side);
    shape.grow(1);
    format!("{} {} {:?}", shape.name(), shape.area(), shape.parts())
}
