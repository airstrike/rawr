//! A second crate beside `events`, as iced_test is beside iced: it
//! re-exports `parts`, which `events` names only through its aliases, as
//! iced_test re-exports iced_core, and doesn't depend on `events`, as
//! iced_test doesn't depend on iced_widget. `parts`'s `widget::Map` has
//! `explain` through `events`'s blanket `Framed`, which no path of this
//! crate reaches.

pub use parts as core;

/// What `map` is, as iced_test's simulator takes what a view gives.
pub fn described<W, F, A>(map: &core::widget::Map<W, F, A>) -> &'static str {
    let _ = map.widget();
    "a map"
}
