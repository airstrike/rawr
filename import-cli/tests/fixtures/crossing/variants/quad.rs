// rawr
// iced's custom_quad and custom_widget: the program's own widget implements
// the crate's widget trait over any message, theme and renderer, bounded by
// the crate's renderer trait, and its `draw` calls the renderer's
// `fill_quad`. rawr compiles the impl once per use the crate's bound makes
// of it (D629), so the body calls the renderer at its type.

use events::custom;
use events::widgets::{button, column, text, Widget};

#[derive(Default)]
struct Counter {
    value: i64,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Increment,
}

struct Quad {
    size: u32,
    radius: u32,
}

impl<Message, Theme, Renderer> custom::Widget<Message, Theme, Renderer> for Quad
where
    Renderer: custom::Renderer,
{
    fn size(&self) -> u32 {
        self.size
    }

    fn draw(&self, renderer: &mut Renderer, _theme: &Theme) {
        renderer.fill_quad(
            custom::Quad {
                size: self.size,
                radius: self.radius,
            },
            events::Color::rgb(1, 2, 3),
        );
    }

    fn press(&self, _label: &str) -> Option<Message> {
        None
    }
}

impl Counter {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => {
                self.value += 1;
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            custom::custom(Quad {
                size: 10 + self.value as u32,
                radius: 2,
            }),
            button("Increment").on_press(Message::Increment),
            text(self.value),
        ]
    }
}

fn main() {
    events::widgets::run(Counter::update, Counter::view);
}
