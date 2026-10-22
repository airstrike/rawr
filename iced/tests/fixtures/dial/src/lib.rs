//! Ordinary unchanged custom iced widget used by Rawr import tests.

use iced::advanced::{self, Layout, Shell};

/// A compact interactive dial with an owned event callback.
pub struct Dial<Message> {
    value: u8,
    step: u8,
    on_change: Box<dyn Fn(u8) -> Message + 'static>,
}

impl<Message> Dial<Message> {
    /// Construct a dial with a current value and owned event callback.
    pub fn new(value: u8, on_change: impl Fn(u8) -> Message + 'static) -> Self {
        Self {
            value,
            step: 1,
            on_change: Box::new(on_change),
        }
    }

    /// Set the increment applied by the next press.
    #[must_use]
    pub fn step(mut self, step: u8) -> Self {
        self.step = step.max(1);
        self
    }

    /// Return the currently displayed value.
    #[must_use]
    pub const fn value(&self) -> u8 {
        self.value
    }
}

impl<Message> advanced::widget::Meta for Dial<Message> {}

impl<Message, Theme, Renderer> advanced::Widget<Message, Theme, Renderer> for Dial<Message>
where
    Renderer: advanced::Renderer,
{
    fn size(&self) -> iced::Size<iced::Length> {
        iced::Size::new(iced::Length::Fixed(36.0), iced::Length::Fixed(36.0))
    }

    fn layout(
        &mut self,
        tree: &mut advanced::widget::Tree,
        _renderer: &Renderer,
        limits: &advanced::layout::Limits,
    ) {
        tree.size = limits.resolve(
            iced::Length::Fixed(36.0),
            iced::Length::Fixed(36.0),
            iced::Size::ZERO,
        );
    }

    fn update(
        &mut self,
        _tree: &mut advanced::widget::Tree,
        event: &iced::Event,
        layout: Layout,
        cursor: advanced::mouse::Cursor,
        _renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        _viewport: &iced::Rectangle,
    ) {
        if matches!(
            event,
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left))
        ) && cursor.is_over(layout.bounds())
        {
            self.value = self.value.saturating_add(self.step);
            shell.publish((self.on_change)(self.value));
            shell.capture_event();
        }
    }

    fn draw(
        &self,
        _tree: &advanced::widget::Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &advanced::renderer::Style,
        layout: Layout,
        _cursor: advanced::mouse::Cursor,
        _viewport: &iced::Rectangle,
    ) {
        renderer.fill_quad(
            advanced::renderer::Quad {
                bounds: layout.bounds(),
                border: iced::Border {
                    color: iced::Color::from_rgb8(0x2f, 0x55, 0x79),
                    width: 2.0,
                    radius: 18.0.into(),
                },
                shadow: iced::Shadow::default(),
                snap: true,
            },
            iced::Color::from_rgb8(self.value, 0x90, 0xc0),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::Dial;
    use iced::widget::Widget as _;

    #[test]
    fn constructor_builder_and_element_conversion_are_ordinary_iced() {
        let dial = Dial::new(4, |value| value).step(3);
        assert_eq!(dial.value(), 4);
        let _element: iced::Element<'_, u8> = dial.boxed();
    }
}
