//! A widget found by a selector, in the shape of iced's `selector`: `find`
//! is generic over a trait with an associated output, which the crate
//! implements for its widget `Id` and for any `FnMut` closure, and a
//! palette whose fields hold the crate's `Color`, as iced's
//! `theme.palette().background.strong.color` does.

use crate::conversions::Id;
use crate::Task;

/// What a selection finds, as iced's `selector::Target`.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    name: String,
}

impl Target {
    /// The width of the widget found, as iced's `Target::visible_bounds`.
    pub fn width(&self) -> Option<f32> {
        Some(self.name.len() as f32 * 10.0)
    }
}

/// A way to find a widget, as iced's `Selector`.
pub trait Selector {
    /// What the selection gives.
    type Output;

    /// The selection among the widget named `name`.
    fn select(&mut self, name: &str) -> Option<Self::Output>;
}

impl Selector for Id {
    type Output = Target;

    fn select(&mut self, name: &str) -> Option<Target> {
        self.name().ends_with(name).then(|| Target {
            name: name.to_owned(),
        })
    }
}

impl<F, T> Selector for F
where
    F: FnMut(&str) -> Option<T>,
{
    type Output = T;

    fn select(&mut self, name: &str) -> Option<T> {
        self(name)
    }
}

/// The first widget `selector` selects, as iced's `selector::find`.
pub fn find<S>(mut selector: S) -> Task<Option<S::Output>>
where
    S: Selector + Send + 'static,
    S::Output: Send + Clone + 'static,
{
    Task(Some(
        ["inner", "outer"]
            .into_iter()
            .find_map(|name| selector.select(name)),
    ))
}

/// One color and its text's, as iced's `palette::Pair`.
#[derive(Debug, Clone, Copy)]
pub struct Pair {
    /// The color.
    pub color: crate::Color,
}

/// A theme's colors, as iced's `palette::Extended`.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    /// The strong one.
    pub strong: Pair,
}

/// The palette, as iced's `theme.palette()`.
pub fn palette() -> Palette {
    Palette {
        strong: Pair {
            color: crate::Color { r: 3, g: 4, b: 5 },
        },
    }
}
