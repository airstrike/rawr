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

/// What `element` shows, at the message, theme and renderer it shows them
/// at, as iced_test's `Simulator<'a, Message, Theme, Renderer>` holds them.
pub struct Sight<Message, Theme, Renderer> {
    shown: String,
    at: std::marker::PhantomData<(Message, Theme, Renderer)>,
}

impl<Message, Theme, Renderer> Sight<Message, Theme, Renderer> {
    /// What it showed.
    pub fn shown(&self) -> &str {
        &self.shown
    }
}

/// What `element` shows, as iced_test's `simulator(element)` takes any
/// core widget: no impl this crate can name fixes `Theme` and `Renderer`
/// of a widget `events` makes, so a view's `impl Shown<Message>` does.
pub fn sight<Message, Theme, Renderer>(
    element: impl core::show::Show<Message, Theme, Renderer>,
) -> Sight<Message, Theme, Renderer> {
    Sight {
        shown: element.shown(),
        at: std::marker::PhantomData,
    }
}
