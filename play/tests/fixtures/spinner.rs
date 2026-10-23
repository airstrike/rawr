use iced::mouse;
use iced::time::{self, Duration};
use iced::widget::canvas::{Geometry, LineCap, Path, Stroke};
use iced::widget::canvas;
use iced::{Color, Point, Rectangle, Renderer, Subscription, Theme, Vector, Widget};

pub fn main() -> iced::Result {
    iced::application(Spinner::default, Spinner::update, Spinner::view)
        .subscription(Spinner::subscription)
        .run()
}

/// Positions in one turn.
const STEPS: u32 = 12;

#[derive(Default)]
struct Spinner {
    step: u32,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Tick,
}

impl Spinner {
    fn update(&mut self, message: Message) {
        match message {
            Message::Tick => self.step = (self.step + 1) % STEPS,
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        time::every(Duration::from_millis(100)).map(|_| Message::Tick)
    }

    fn view(&self) -> impl Widget<Message> {
        canvas(self).width(120).height(120)
    }
}

impl<Message> canvas::Program<Message> for Spinner {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let center = frame.center();
        frame.translate(Vector::new(center.x, center.y));
        frame.rotate(self.step as f32 * std::f32::consts::PI * 2.0 / STEPS as f32);
        frame.stroke(
            &Path::line(Point::new(0.0, 0.0), Point::new(0.0, -48.0)),
            Stroke::default()
                .with_width(8.0)
                .with_color(Color::from_rgb8(0xff, 0x2e, 0x93))
                .with_line_cap(LineCap::Round),
        );
        frame.fill(&Path::circle(Point::new(0.0, 0.0), 8.0), Color::WHITE);
        vec![frame.into_geometry()]
    }
}
