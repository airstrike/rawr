// rawr
//! The home screen, marked, as inboard's `workspace/screen/home.rs`: the
//! native workspace keeps its `State`, whose editor content can't be
//! cloned, between calls, and its `update` gives the crate's tasks over
//! its own `Message`, mapping its native child's with a variant.

use ui::{Action, Content, Element, Task};

use super::inbox;

pub mod wizard;

const STEP: u32 = 1;

#[derive(Debug, Clone)]
pub enum Message {
    Edited(Action),
    Inbox(inbox::Message),
    Wizard(wizard::Message),
    Count,
}

#[derive(Debug)]
pub struct State {
    content: Content,
    count: u32,
    title: String,
    greeting: String,
}

impl State {
    pub fn new() -> Self {
        Self {
            content: Content::default(),
            count: 0,
            title: "home".to_owned(),
            greeting: "hello".to_owned(),
        }
    }

    pub fn count(&self) -> u32 {
        self.count
    }

    pub fn text(&self) -> String {
        self.content.text()
    }

    /// Open the wizard at the current count, as inboard's `edit_tile`.
    pub fn open(&mut self) -> Task<Message> {
        self.count += STEP;
        wizard::start(self.count).map(Message::Wizard)
    }

    /// The screen, as inboard's `view<'a>(&'a self, ..)`: an element that
    /// borrows its own text for `'a`, with the native inbox it is lent
    /// read, and a child's element mapped into its messages.
    pub fn view<'a>(&'a self, inbox: &'a inbox::State) -> Element<'a, Message> {
        if self.count > 3 {
            ui::button(&self.title, Message::Count)
        } else if inbox.unread > 3 {
            ui::button(&self.greeting, inbox::Message::Read(inbox.unread)).map(Message::Inbox)
        } else {
            wizard::view(&self.title, self.count).map(Message::Wizard)
        }
    }

    pub fn summary(&self) -> String {
        format!(
            "{} after {} edits, count {}",
            self.content.text(),
            self.content.edits(),
            self.count
        )
    }
}

pub fn update(state: &mut State, message: Message, inbox: &mut inbox::State) -> Task<Message> {
    match message {
        Message::Edited(action) => {
            state.content.perform(action);
            Task::none()
        }
        Message::Inbox(message) => inbox.update(message).map(Message::Inbox),
        Message::Wizard(wizard::Message::Step(step)) if step > 2 => Task::done(Message::Count),
        Message::Wizard(wizard::Message::Step(step)) => {
            state.count += step;
            Task::none()
        }
        Message::Count => {
            state.count += STEP;
            Task::done(Message::Edited(Action::Insert('!')))
        }
    }
}

pub fn view(state: &State) -> Element<'_, Message> {
    if state.count > 3 {
        ui::button("count", Message::Count)
    } else if state.count > 1 {
        ui::button(&state.greeting, wizard::Message::Step(state.count)).map(Message::Wizard)
    } else {
        ui::button("wizard", wizard::Message::Step(state.count)).map(Message::Wizard)
    }
}
