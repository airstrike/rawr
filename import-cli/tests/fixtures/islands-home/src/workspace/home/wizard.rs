//! Native: a child of the marked `home`, whose tasks and elements `home`
//! maps into its own messages.

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Step(u32),
}

pub fn start(count: u32) -> ui::Task<Message> {
    ui::Task::done(Message::Step(count))
}

/// The wizard's element, which shows `label` and borrows it.
pub fn view(label: &str, count: u32) -> ui::Element<'_, Message> {
    ui::button(label, Message::Step(count))
}
