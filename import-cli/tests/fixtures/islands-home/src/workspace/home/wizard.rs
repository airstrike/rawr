//! Native: a child of the marked `home`, whose tasks `home` maps into its
//! own messages.

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Step(u32),
}

pub fn start(count: u32) -> ui::Task<Message> {
    ui::Task::done(Message::Step(count))
}
