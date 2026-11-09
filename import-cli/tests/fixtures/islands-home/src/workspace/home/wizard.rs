//! Native: a child of the marked `home`, whose tasks `home` maps into its
//! own messages, and whose state, which can't be cloned, `home` keeps in
//! an `Option` while it is open.

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Step(u32),
}

pub fn start(count: u32) -> ui::Task<Message> {
    ui::Task::done(Message::Step(count))
}

/// The open wizard: it can't be cloned, and says when it closes.
#[derive(Debug)]
pub struct State {
    step: u32,
}

impl State {
    pub fn new(step: u32) -> Self {
        Self { step }
    }

    pub fn step(&self) -> u32 {
        self.step
    }

    pub fn advance(&mut self) {
        self.step += 1;
    }
}

impl Drop for State {
    fn drop(&mut self) {
        println!("wizard closed at step {}", self.step);
    }
}
