// rawr
//! `?play`'s universe (#434, #436). The prebuilt runner covers the native
//! calls this program makes, and a snippet runs on it when its own native
//! calls are among them. Script types are named by their index in the
//! program's definition order, so a snippet's first two types meet this
//! program's `Counter` and `Message` whatever their names.

use iced::widget::{button, column, text, Widget};
use iced::Center;
use std::time::Duration;

#[derive(Default)]
struct Counter {
    value: i64,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Increment,
    Decrement,
}

impl Counter {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => {
                self.value += 1;
            }
            Message::Decrement => {
                self.value -= 1;
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            button("Increment").on_press(Message::Increment),
            text(self.value).size(50),
            button("Decrement").on_press(Message::Decrement)
        ]
        .padding(20)
        .align_x(Center)
    }
}

/// An iced program's `main`, as iced's counter writes it.
pub fn run() -> iced::Result {
    iced::run(Counter::update, Counter::view)
}

#[tokio::main]
async fn main() {
    tokio::spawn(async { println!("ran") });
    tokio::time::sleep(Duration::from_millis(50)).await;
    let handle = tokio::spawn(async { 6 * 7 });
    let value = handle.await.unwrap();
    println!("joined {value}");
    let _blocking = tokio::task::spawn_blocking(|| println!("blocking ran"));
    tokio::time::sleep(Duration::from_millis(50)).await;
}
