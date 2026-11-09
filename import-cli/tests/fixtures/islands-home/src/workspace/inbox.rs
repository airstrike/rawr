//! Native: the workspace's inbox, whose state can't be cloned and which
//! the marked home screen updates through a `&mut`.

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Read(usize),
}

#[derive(Debug, Default)]
pub struct State {
    pub unread: usize,
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
}
