// rawr
use iced::Color;
use iced::Element;
use rawr_iced::widget::event::publish;
use rawr_iced::widget::layout::Size;

#[derive(Default)]
pub struct State {
    clicks: i64,
}

#[derive(Default)]
pub struct App {
    radius: i64,
}

struct Dial {
    radius: i64,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Changed(i64),
}

#[widget]
impl Dial {
    fn state(&self) -> State {
        State { clicks: 0 }
    }

    fn size(&self) -> Size {
        Size::fixed(40, 40)
    }

    fn layout(
        &mut self,
        state: &mut State,
        input: rawr_iced::widget::layout::Input,
    ) -> rawr_iced::widget::layout::Node {
        input.resolve(Size::fixed(40, 40), 40, 40)
    }

    fn draw(
        &self,
        state: &State,
        renderer: &mut rawr_iced::Renderer,
        input: rawr_iced::widget::draw::Input,
    ) {
        renderer.fill_quad(input.bounds(), Color::from_rgb(0.95, 0.45, 0.10));
    }

    fn update(
        &mut self,
        state: &mut State,
        input: rawr_iced::widget::event::Input,
    ) -> rawr_iced::widget::event::Outcome<Message> {
        input.is_press();
        state.clicks = 1;
        publish(Message::Changed(1))
    }
}

impl App {
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Changed(radius) => self.radius = radius,
        }
    }

    pub fn view(&self) -> Element<Message> {
        Dial { radius: self.radius }
    }
}
