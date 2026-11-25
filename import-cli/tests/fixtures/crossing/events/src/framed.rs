//! Frames: a crate's generic type and its impl of the crate's own trait,
//! which the crate's concrete code calls through a trait object, as
//! iced's `Container<'a, W, Theme>` is a `Widget` an `Element` boxes. The
//! trait has a supertrait, as `Widget` has `widget::Meta`.

/// What a cell knows of itself whatever it shows.
pub trait Meta {
    /// Whether the cell shows nothing, which the crate's concrete code
    /// leaves out.
    fn is_void(&self) -> bool {
        false
    }
}

/// What a cell shows. The crate's concrete code calls it through a trait
/// object.
pub trait Shown: Meta {
    /// The cell's text.
    fn shown(&self) -> String;

    /// The cell held as its trait object, as iced's `Widget::_boxed`
    /// boxes a widget for `column!`.
    fn held<'a>(self) -> crate::held::Held<'a>
    where
        Self: Sized + 'a,
    {
        crate::held::Held::new(self)
    }
}

/// A label the crate shows as it is.
#[derive(Debug, Clone)]
pub struct Label(pub String);

impl Meta for Label {}

impl Label {
    /// The label padded to `columns`, as iced's `Text::size` builds a text
    /// of a size: an inherent builder whose name a trait's method shares.
    #[must_use]
    pub fn width(self, columns: usize) -> Self {
        Self(format!("{:columns$}", self.0))
    }
}

impl Shown for Label {
    fn shown(&self) -> String {
        self.0.clone()
    }
}

/// `item` between two edges. Its fields are private.
pub struct Framed<S> {
    item: S,
    edge: char,
}

/// `item` framed by bars.
pub fn framed<S: Shown>(item: S) -> Framed<S> {
    Framed { item, edge: '|' }
}

impl<S> Framed<S> {
    /// The frame with `edge` on both sides.
    #[must_use]
    pub fn edge(mut self, edge: char) -> Self {
        self.edge = edge;
        self
    }
}

impl<S> Meta for Framed<S> {}

impl<S: Shown> Shown for Framed<S> {
    fn shown(&self) -> String {
        format!("{}{}{}", self.edge, self.item.shown(), self.edge)
    }
}

/// `item` shown through its borrowed trait object, by the crate's
/// concrete code.
#[must_use]
pub fn shown_one(item: &dyn Shown) -> String {
    format!("<{}>", item.shown())
}

/// Each of `items` that shows something, shown through its trait object,
/// by the crate's concrete code.
#[must_use]
pub fn shown_all(items: &[Box<dyn Shown>]) -> String {
    items
        .iter()
        .filter(|item| !item.is_void())
        .map(|item| item.shown())
        .collect::<Vec<_>>()
        .join(" ")
}

/// What paints a frame's edges, as iced's `container::Catalog` styles a
/// container: a class per frame, which a frame holds by the palette's
/// associated type.
pub trait Palette {
    /// A frame's class, as iced's `Catalog::Class<'a>`.
    type Class<'a>;

    /// The class a frame starts with, as iced's `Catalog::default`.
    fn default<'a>() -> Self::Class<'a>;

    /// The edge this palette paints.
    fn edge(&self) -> char;

    /// The edge `class` paints by this palette, as iced's `Catalog::style`.
    fn paint(&self, class: &Self::Class<'_>) -> char;
}

/// The crate's palette.
pub struct Ink;

/// A class that paints by a closure of the palette, as iced's
/// `container::StyleFn<'a, Theme>`.
pub type EdgeFn<'a, P> = Box<dyn Fn(&P) -> char + 'a>;

impl Palette for Ink {
    type Class<'a> = EdgeFn<'a, Self>;

    fn default<'a>() -> EdgeFn<'a, Self> {
        Box::new(|_| '+')
    }

    fn edge(&self) -> char {
        '|'
    }

    /// Names the class by the trait's projection, as iced's `impl Catalog
    /// for Theme` writes `class: &Self::Class<'_>`: rustc normalizes it
    /// to `&EdgeFn<'_, Ink>` at the signature's own lifetime.
    fn paint(&self, class: &Self::Class<'_>) -> char {
        class(self)
    }
}

/// `item` between two edges its palette paints, in the shape of iced's
/// `Container<'a, W, Theme>`: it holds its palette's class. Its fields are
/// private.
pub struct Painted<'a, S, P = Ink>
where
    P: Palette,
{
    item: S,
    palette: P,
    class: P::Class<'a>,
    classed: bool,
}

impl<'a, S, P> Painted<'a, S, P>
where
    P: Palette,
{
    /// `item` painted by `palette` with `class`.
    pub fn new(item: S, palette: P, class: P::Class<'a>) -> Self {
        Self {
            item,
            palette,
            class,
            classed: false,
        }
    }

    /// `item` painted by `palette` with its default class, as iced's
    /// `Container::new` takes `Theme::default()`.
    pub fn plain(item: S, palette: P) -> Self {
        Self::new(item, palette, P::default())
    }

    /// The edge the frame's class paints, as iced's `Container::draw`
    /// styles it by `theme.style(&self.class)`.
    pub fn painted(&self) -> char {
        self.palette.paint(&self.class)
    }

    /// The frame with the class `edge`, as iced's `Container::style` sets
    /// one through a bound on the class.
    #[must_use]
    pub fn class(mut self, edge: impl Fn(&P) -> char + 'a) -> Self
    where
        P::Class<'a>: From<EdgeFn<'a, P>>,
    {
        self.class = (Box::new(edge) as EdgeFn<'a, P>).into();
        self.classed = true;
        self
    }
}

impl<S, P> Meta for Painted<'_, S, P> where P: Palette {}

impl<S: Shown, P> Shown for Painted<'_, S, P>
where
    P: Palette,
{
    fn shown(&self) -> String {
        let edge = self.palette.edge();
        let mark = if self.classed { "*" } else { "" };
        format!("{edge}{mark}{}{mark}{edge}", self.item.shown())
    }
}

/// What a cell is drawn onto, as iced's `Renderer`: a crate type that
/// native code makes and lends.
#[derive(Debug, Default)]
pub struct Canvas {
    marks: String,
}

impl Canvas {
    /// Marks `text` on the canvas.
    pub fn mark(&mut self, text: &str) {
        self.marks.push_str(text);
    }

    /// What the canvas is marked with.
    #[must_use]
    pub fn marks(&self) -> &str {
        &self.marks
    }
}

/// What draws a cell onto a canvas its parameter names, as iced's
/// `Widget<Message, Theme, Renderer>::draw` takes `&mut Renderer`.
pub trait Drawn<C> {
    /// Draws the cell onto `canvas`.
    fn draw(&self, canvas: &mut C);

    /// The cell held as its trait object, as iced's `Widget::_boxed`
    /// makes an `Element` at the widget's own parameters.
    fn drawing<'a>(self) -> crate::held::Drawing<'a, C>
    where
        Self: Sized + 'a,
    {
        crate::held::Drawing(Box::new(self))
    }
}

impl Drawn<Canvas> for Label {
    fn draw(&self, canvas: &mut Canvas) {
        canvas.mark(&self.0);
    }
}

impl<S: Drawn<Canvas>> Drawn<Canvas> for Framed<S> {
    fn draw(&self, canvas: &mut Canvas) {
        let edge = self.edge.to_string();
        canvas.mark(&edge);
        self.item.draw(canvas);
        canvas.mark(&edge);
    }
}

/// Each of `items` drawn onto one canvas through its trait object, by
/// the crate's concrete code.
#[must_use]
pub fn drawn_all(items: &[Box<dyn Drawn<Canvas>>]) -> String {
    let mut canvas = Canvas::default();
    for item in items {
        item.draw(&mut canvas);
        canvas.mark(" ");
    }
    canvas.marks
}

/// Each of `items` after its place, by `parts`' generic `tallied`, as
/// iced's `Container::diff` calls iced_core's `Tree::diff_children`.
pub fn tallied<T: std::fmt::Display>(items: &[T]) -> String {
    crate::tally::tallied(items)
}
