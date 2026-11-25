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
}

/// A label the crate shows as it is.
#[derive(Debug, Clone)]
pub struct Label(pub String);

impl Meta for Label {}

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
