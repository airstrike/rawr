use iced::animation::Easing;
use iced::time::{milliseconds, Instant};
use iced::widget::{button, column, progress_bar, text};
use iced::{window, Animation, Subscription, Widget};

pub fn main() -> iced::Result {
    iced::application::timed(Fill::new, Fill::update, Fill::subscription, Fill::view).run()
}

struct Fill {
    progress: Animation<bool>,
    now: Instant,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Toggle,
    Frame,
}

impl Fill {
    fn new() -> Self {
        Self {
            progress: Animation::new(false)
                .duration(milliseconds(1000))
                .easing(Easing::EaseInOut),
            now: Instant::now(),
        }
    }

    // `timed` passes the Instant of each message, so `update` never reads
    // the clock itself.
    fn update(&mut self, message: Message, now: Instant) {
        self.now = now;
        match message {
            Message::Toggle => self.progress.go_mut(!self.progress.value(), now),
            Message::Frame => {}
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        if self.progress.is_animating(self.now) {
            window::frames().map(|_| Message::Frame)
        } else {
            Subscription::none()
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let value = self.progress.interpolate(0.0, 100.0, self.now);
        column![
            text(format!("{value:.0}%")).size(24),
            progress_bar(0.0..=100.0, value).length(240),
            button("Toggle").on_press(Message::Toggle),
        ]
        .spacing(8)
    }
}
