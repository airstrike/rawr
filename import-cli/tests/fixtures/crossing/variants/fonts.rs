// rawr
// iced's text: `new` lists the font families, a `Task<Result<Vec<Family>,
// Error>>` of a crate enum mapped to a message on another thread, and
// reads the latest window through the bounded generic `Task::done` as
// `and_then` takes it; `listen_with` takes a closure as its function
// pointer, whose `Option` output crosses back; and `pick(..)` takes a
// slice of the crate enum, shown by its `Display` as `Family::to_string`,
// which borrows each option.

use events::controls::{caption, listen_with, pick, Signal, Subscription};
use events::widgets::{column, Widget};
use events::{families, latest, Error, Family, Task};

struct Fonts {
    family: Family,
    families: Vec<Family>,
    scale: f32,
    window: u32,
}

#[derive(Debug, Clone)]
enum Message {
    FamilyChanged(Family),
    FamiliesListed(Result<Vec<Family>, Error>),
    Rescaled(f32),
}

impl Fonts {
    fn new() -> (Self, Task<Message>) {
        let window = latest().and_then(Task::done);
        (
            Self {
                family: Family::SansSerif,
                families: vec![Family::SansSerif],
                scale: 1.0,
                window: window.value().unwrap_or(0),
            },
            families().map(Message::FamiliesListed),
        )
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::FamilyChanged(family) => {
                self.family = family;
            }
            Message::FamiliesListed(listed) => {
                if let Ok(families) = listed {
                    self.families = families;
                }
            }
            Message::Rescaled(scale) => {
                self.scale = scale;
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        listen_with(|signal, _, _| {
            let Signal::Rescaled(scale) = signal else {
                return None;
            };

            Some(Message::Rescaled(scale))
        })
    }

    fn view(&self) -> impl Widget<Message> {
        let selector = pick(
            Some(self.family),
            self.families.as_slice(),
            Family::to_string,
        )
        .on_select(Message::FamilyChanged);
        column![
            selector,
            caption(format!(
                "{} at {} in {}",
                self.family, self.scale, self.window
            ))
        ]
    }
}

fn main() {
    events::controls::application(Fonts::new, Fonts::update, Fonts::view)
        .subscription(Fonts::subscription)
        .run()
}
