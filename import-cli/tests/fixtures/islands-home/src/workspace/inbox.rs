//! Native: the workspace's inbox, whose state can't be cloned, which the
//! marked home screen updates through a `&mut` and whose element it shows
//! from the `&` it is lent, as inboard's `inbox.view()`.

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Read(usize),
}

#[derive(Debug)]
pub struct State {
    pub unread: usize,
    greeting: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            unread: 0,
            greeting: "hello".to_owned(),
        }
    }
}

impl State {
    pub fn update(&mut self, message: Message) -> ui::Task<Message> {
        match message {
            Message::Read(read) => {
                self.unread = self.unread.saturating_sub(read);
                ui::Task::none()
            }
        }
    }

    /// The inbox's element, which borrows its greeting.
    pub fn view(&self) -> ui::Element<'_, Message> {
        ui::button(&self.greeting, Message::Read(self.unread))
    }
}
