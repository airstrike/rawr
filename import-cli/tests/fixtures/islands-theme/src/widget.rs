//! Native: the app's widgets, as inboard's `widget`, with a `text` of its
//! own that a marked module imports beside `theme`, an `Element` alias
//! over the marked `Theme`, as inboard's `widget::Element` is, and the
//! crate's generic builders over that theme.
use crate::Theme;

pub use ui::vertical_space;

pub type Renderer = ui::Renderer;
pub type Element<'a, Message> = ui::Element<'a, Message, Theme, Renderer>;
pub type Scrollable<'a, Message> = ui::Scrollable<'a, Message, Element<'a, Message>, Theme>;

// Builders the islands-theme test adds to the marked `sidebar` use these.
#[allow(unused_imports)]
pub use ui::mouse_area;

#[allow(dead_code)]
pub type Button<'a, Message> = ui::button::Button<'a, Message, Theme>;
pub type Container<'a, Message> = ui::Container<'a, Element<'a, Message>, Theme>;
pub type TextInput<'a, Message> = ui::TextInput<'a, Message, Theme>;

/// A label over any fragment, as inboard's `widget::text(impl
/// IntoFragment<'a>)`.
pub fn label<'a>(content: impl ui::text::IntoFragment<'a>) -> ui::Label<'a, Theme> {
    ui::label(&content.into_fragment())
}

/// A text input over the theme, as inboard's `sweeten::text_input`.
pub fn text_input<'a, Message>(placeholder: &str, value: &str) -> TextInput<'a, Message> {
    ui::text_input(placeholder, value)
}

pub fn text(content: &str) -> String {
    format!("text {content}")
}

/// An element of nothing, as inboard's `widget::void`.
pub fn void<'a, Message: 'a>() -> Element<'a, Message> {
    use ui::Widget;
    ui::label("void").boxed()
}

/// Content that scrolls, as inboard's `widget::scrollable`.
pub fn scrollable<'a, Message>(
    content: impl ui::Widget<Message, Theme, Renderer> + 'a,
) -> Scrollable<'a, Message> {
    ui::scrollable(content.boxed())
}
