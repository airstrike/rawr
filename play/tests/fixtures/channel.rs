use iced::widget::text;
use iced::{Task, Widget};
use std::time::Duration;
use tokio::sync::mpsc;

pub fn main() -> iced::Result {
    iced::application(Sum::start, Sum::update, Sum::view).run()
}

#[derive(Default)]
struct Sum {
    total: Option<u32>,
}

#[derive(Debug, Clone)]
enum Message {
    Total(u32),
}

async fn total() -> u32 {
    let (sender, mut receiver) = mpsc::channel(2);
    let producer = async move {
        for n in 1..=4 {
            sender.send(n).await.unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    };
    let consumer = async move {
        let mut sum = 0;
        while let Some(n) = receiver.recv().await {
            sum += n;
        }
        sum
    };
    let (_, sum) = tokio::join!(producer, consumer);
    sum
}

impl Sum {
    fn start() -> (Self, Task<Message>) {
        (Self::default(), Task::perform(total(), Message::Total))
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::Total(total) => self.total = Some(total),
        }
    }

    fn view(&self) -> impl Widget<Message> {
        text(match self.total {
            Some(total) => format!("sum {total}"),
            None => "adding up".to_string(),
        })
    }
}