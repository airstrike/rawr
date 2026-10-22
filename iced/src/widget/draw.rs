//! Owned drawing facts supplied to one scripted widget invocation.

/// Validated drawing inputs supplied only by the iced adapter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Input {
    bounds: iced::Rectangle,
    cursor: Option<iced::Point>,
    viewport: iced::Rectangle,
    text_color: iced::Color,
}

impl Input {
    pub(crate) const fn new(
        bounds: iced::Rectangle,
        cursor: Option<iced::Point>,
        viewport: iced::Rectangle,
        text_color: iced::Color,
    ) -> Self {
        Self {
            bounds,
            cursor,
            viewport,
            text_color,
        }
    }

    pub(crate) const fn bounds(self) -> iced::Rectangle {
        self.bounds
    }

    #[cfg(test)]
    pub(crate) const fn cursor(self) -> Option<iced::Point> {
        self.cursor
    }

    pub(crate) const fn viewport(self) -> iced::Rectangle {
        self.viewport
    }

    pub(crate) const fn text_color(self) -> iced::Color {
        self.text_color
    }
}
