use iced::animation::Easing;
use iced::time::{Instant, milliseconds};
use iced::widget::{column, progress_bar, text};
use iced::{Animation, Subscription, Widget, window};

pub fn main() -> iced::Result {
    iced::application(Fill::new, Fill::update, Fill::view)
        .subscription(Fill::subscription)
        .run()
}

struct Fill {
    progress: Animation<bool>,
    now: Instant,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Frame(Instant),
}

impl Fill {
    fn new() -> Self {
        let now = Instant::now();
        let mut progress = Animation::new(false)
            .duration(milliseconds(1000))
            .easing(Easing::EaseInOut);
        progress.go_mut(true, now);
        Self { progress, now }
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::Frame(now) => self.now = now,
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        if self.progress.is_animating(self.now) {
            window::frames().map(Message::Frame)
        } else {
            Subscription::none()
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let value = self.progress.interpolate(0.0, 100.0, self.now);
        column![
            text(format!("{value:.0}%")).size(24),
            progress_bar(0.0..=100.0, value).length(240),
        ]
        .spacing(8)
    }
}
