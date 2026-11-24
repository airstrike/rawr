//! Tints and canvases: concrete code the crate compiles once, and generic
//! code that each program instantiates at its own types, as iced's
//! `container::background(impl Into<Background>)` is.

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::paint::Paint;

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
pub fn coats(paint: impl Into<Paint>) -> u8 {
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
