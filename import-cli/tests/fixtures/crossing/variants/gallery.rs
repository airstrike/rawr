// rawr
// iced's gallery: `card(..).boxed()` inside an iterator's `map` closure
// over the program's own native images. `card` borrows an image and the
// preview an `if` chooses from a map, gives `impl Widget<Message> + 'a`,
// and is written after the view. `.chain(..)` boxes placeholders from a
// range an `if` chooses, and `Column::with_children` takes the chained
// elements (#570).

mod download;

use std::collections::HashMap;

use download::Image;
use events::widgets::{button, column, text, Column, Widget};

#[derive(Default)]
struct Gallery {
    images: Vec<Image>,
    previews: HashMap<u32, String>,
}

#[derive(Debug, Clone)]
enum Message {
    Increment,
    Decrement,
}

impl Gallery {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => {
                let id = self.images.len() as u32;
                self.images.push(Image {
                    id,
                    url: "image".to_owned(),
                });
                self.previews.insert(id, format!("preview{id}"));
            }
            Message::Decrement => {
                self.images.pop();
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let images = self
            .images
            .iter()
            .map(|image| {
                card(
                    image,
                    if image.id % 2 == 0 {
                        self.previews.get(&image.id)
                    } else {
                        None
                    },
                    self.images.len(),
                )
                .boxed()
            })
            .chain(
                if self.images.is_empty() {
                    0..Image::LIMIT
                } else {
                    0..0
                }
                .map(|_| placeholder().boxed()),
            );

        column![
            button("Increment").on_press(Message::Increment),
            Column::with_children(images).padding(1),
        ]
    }
}

fn card<'a>(
    metadata: &'a Image,
    preview: Option<&'a String>,
    count: usize,
) -> impl Widget<Message> + 'a {
    let card = button(metadata).on_press(if count > 2 {
        Message::Decrement
    } else {
        Message::Increment
    });

    let card = if let Some(preview) = preview {
        column![card, text(preview)].boxed()
    } else {
        card.boxed()
    };

    Column::with_children([card]).padding(2)
}

fn placeholder() -> impl Widget<Message> {
    text("...")
}

fn main() {
    events::widgets::run(Gallery::update, Gallery::view);
}
