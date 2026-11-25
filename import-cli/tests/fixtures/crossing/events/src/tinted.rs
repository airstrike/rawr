//! Tints and canvases: concrete code the crate compiles once, and generic
//! code that each program instantiates at its own types, as iced's
//! `container::background(impl Into<Background>)` is.

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::framed::Shown;

/// A colour whose channels are public.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tint {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl From<u8> for Tint {
    fn from(gray: u8) -> Self {
        Self {
            red: gray,
            green: gray,
            blue: gray,
        }
    }
}

impl From<[u8; 3]> for Tint {
    fn from([red, green, blue]: [u8; 3]) -> Self {
        Self { red, green, blue }
    }
}

/// A shaded tint and the order the crate shaded it in. Its fields are
/// private.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Style {
    tint: Tint,
    order: usize,
}

impl Style {
    /// The shaded tint.
    #[must_use]
    pub fn tint(&self) -> Tint {
        self.tint
    }

    /// Which shading of the crate's this style was, from 1.
    #[must_use]
    pub fn order(&self) -> usize {
        self.order
    }

    /// `item`'s text after this style's order, through the crate trait
    /// the module imports by its single name.
    #[must_use]
    pub fn labelled(&self, item: &impl Shown) -> String {
        format!("{}:{}", self.order, item.shown())
    }

    /// The style with `tint`, darkened, in the same order.
    #[must_use]
    pub fn retinted(self, tint: impl Into<Tint>) -> Self {
        Self {
            tint: darken(tint.into()),
            ..self
        }
    }
}

/// A count that only goes up. Its count is private.
struct Counter {
    count: AtomicUsize,
}

impl Counter {
    /// The next count, from 1.
    fn bump(&self) -> usize {
        self.count.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// The count so far.
    fn get(&self) -> usize {
        self.count.load(Ordering::SeqCst)
    }
}

/// How many styles the crate has shaded, by its own code or a program's.
static SHADED: Counter = Counter {
    count: AtomicUsize::new(0),
};

/// `tint`, darkened by a quarter.
fn darken(tint: Tint) -> Tint {
    let channel = |value: u8| value - value / 4;
    Tint {
        red: channel(tint.red),
        green: channel(tint.green),
        blue: channel(tint.blue),
    }
}

/// How many styles the crate has shaded so far.
#[must_use]
pub fn shaded() -> usize {
    SHADED.get()
}

/// White, shaded by the crate's own concrete code.
#[must_use]
pub fn plain() -> Style {
    let order = SHADED.bump();
    Style {
        tint: darken(Tint::from(255)),
        order,
    }
}

/// `tint`, darkened and counted.
pub fn shade(tint: impl Into<Tint>) -> Style {
    let order = SHADED.bump();
    Style {
        tint: darken(tint.into()),
        order,
    }
}

/// How many coats `paint` takes, through the `From` of a module apart
/// from this one (`crate::paint`).
pub fn coats(paint: impl Into<crate::paint::Paint>) -> u8 {
    paint.into().coats
}

/// The cells items are drawn into. They are private.
#[derive(Default)]
pub struct Canvas {
    cells: Vec<String>,
}

impl Canvas {
    /// An empty canvas.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A cell showing `label` in `style`.
    pub fn fill(&mut self, label: &str, style: &Style) {
        let Tint { red, green, blue } = style.tint;
        self.cells.push(format!(
            "{label} #{red:02x}{green:02x}{blue:02x} @{}",
            style.order
        ));
    }

    /// The cells, in the order they were filled.
    #[must_use]
    pub fn cells(&self) -> &[String] {
        &self.cells
    }
}

/// Each of `items` shaded into a cell of `canvas` labelled `title`.
pub fn draw<M: Clone + Into<Tint>>(title: &str, items: &[M], canvas: &mut Canvas) {
    for (index, item) in items.iter().enumerate() {
        let label = format!("{title} {index}");
        canvas.fill(&label, &shade(item.clone()));
    }
}

/// The labels of what a holder shows, as iced's `widget::Tree` keeps a
/// widget's children. The crate compiles it once.
#[derive(Debug, Default)]
pub struct Tally {
    pub labels: Vec<String>,
}

impl Tally {
    /// An empty tally.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The tally's labels made `items`', as iced's `Tree::diff_children`.
    pub fn diff_children(&mut self, items: &mut [impl Shown]) {
        diff_children(&mut self.labels, items);
    }
}

/// `labels` made the labels of `items`, as iced's `tree::diff_children`:
/// the extra ones dropped and each of the rest relabelled in place.
pub fn diff_children(labels: &mut Vec<String>, items: &mut [impl Shown]) {
    labels.truncate(items.len());
    for (index, item) in items.iter_mut().enumerate() {
        let shown = item.shown();
        if index < labels.len() {
            labels[index] = shown;
        } else {
            labels.push(shown);
        }
    }
}

/// `item` with a tally of it, as iced's `Container` holds its content.
pub struct Tallied<S> {
    item: S,
    tally: Tally,
}

/// `item` with an empty tally.
pub fn tallied<S>(item: S) -> Tallied<S> {
    Tallied {
        item,
        tally: Tally::new(),
    }
}

impl<S: Shown> Tallied<S> {
    /// The tally made the one item's, as iced's `Container::diff` diffs its
    /// content through `std::slice::from_mut`.
    pub fn diffed(&mut self) -> &[String] {
        self.tally
            .diff_children(std::slice::from_mut(&mut self.item));
        &self.tally.labels
    }
}

/// What a holder diffs, as iced's `Widget<Message, Theme, Renderer>`,
/// whose `Message` only the impls and their bounds name.
pub trait Diffed<M> {
    /// The holder's label, made as it diffs.
    fn label(&mut self) -> String;

    /// How many columns the holder takes, as iced's `Widget::size`.
    fn width(&self) -> usize {
        0
    }

    /// Notes the holder's labels in `visit`, as iced's `Widget::operate`
    /// takes a `&mut dyn Operation`.
    fn operate(&mut self, _visit: &mut dyn crate::visits::Visit) {}
}

/// A crate label diffs at any `M`, as iced's `impl<Message, ..> Widget<
/// Message, ..> for Text` is a widget at any message.
impl<M> Diffed<M> for crate::framed::Label {
    fn label(&mut self) -> String {
        self.0.clone()
    }

    fn width(&self) -> usize {
        self.0.len()
    }

    fn operate(&mut self, visit: &mut dyn crate::visits::Visit) {
        visit.visit(&self.0);
    }
}

impl Tally {
    /// The tally's labels made `items`', as `diff_children`, at the `M`
    /// only the items' bound names.
    pub fn diff_marked<M>(&mut self, items: &mut [impl Diffed<M>]) {
        self.labels.clear();
        for item in items.iter_mut() {
            self.labels.push(item.label());
        }
    }
}

impl<S, M> Diffed<M> for Tallied<S>
where
    S: Diffed<M>,
{
    fn label(&mut self) -> String {
        self.tally.diff_marked(std::slice::from_mut(&mut self.item));
        format!("[{}]", self.tally.labels.join(","))
    }

    /// The item's columns and two edges, through the item's bound: the
    /// generic body reads `self.item` as an `S`, so its `width` is
    /// `Diffed`'s whatever inherent `width` the instance's type has, as
    /// iced's `Container::diff` reads `self.content.size()`.
    fn width(&self) -> usize {
        self.item.width() + 2
    }

    /// Notes itself, then the item one level deeper, as iced's
    /// `Container::operate` passes its content the operation through
    /// `operation.traverse(&mut |operation| ..)`.
    fn operate(&mut self, visit: &mut dyn crate::visits::Visit) {
        visit.visit("tallied");
        visit.traverse(&mut |visit| self.item.operate(visit));
    }
}
