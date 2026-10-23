#[derive(Default)]
struct Counter {
    value: i64,
}

enum Message {
    Increment,
    Decrement,
}

impl Counter {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => self.value += 1,
            Message::Decrement => self.value -= 1,
        }
    }

    fn value(&self) -> i64 {
        self.value
    }
}

let mut counter = Counter { value: 41 };
counter.update(Message::Increment);
counter.update(Message::Increment);
counter.update(Message::Decrement);
counter.value()
