//! Native: the workspace's inbox, whose state can't be cloned, which the
//! marked home screen updates through a `&mut` and whose element it shows
//! from the `&` it is lent, as inboard's `inbox.view()`, or through the
//! free `view`, which keeps the borrows it is given, as inboard's
//! `inbox::view(inbox_state, .., browser_updates, ..)`.

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Read(usize),
}

/// It caches what it read in a `RefCell`, as inboard's `model::Document`
/// does, so a `&State` can't cross threads.
#[derive(Debug)]
pub struct State {
    pub unread: usize,
    greeting: String,
    read: std::cell::RefCell<Vec<usize>>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            unread: 0,
            greeting: "hello".to_owned(),
            read: std::cell::RefCell::new(Vec::new()),
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

    pub fn greeting(&self) -> &str {
        self.read.borrow_mut().push(self.unread);
        &self.greeting
    }
}

/// An extension's update, as inboard's `extension::registry::Update`: it
/// can't be cloned.
#[derive(Debug)]
pub struct Update {
    name: String,
}

impl Update {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
        }
    }
}

/// The inbox's top bar, as inboard's free `inbox::view`: it borrows the
/// first update's name, or the state's greeting when there is none.
pub fn view<'a>(state: &'a State, updates: &'a [Update]) -> ui::Element<'a, Message> {
    match updates.first() {
        Some(update) => ui::button(&update.name, Message::Read(state.unread)),
        None => ui::button(&state.greeting, Message::Read(state.unread)),
    }
}

/// How many wait, as a crate fn whose output keeps neither the state nor
/// the updates it is lent.
pub fn pending(state: &State, updates: &[Update]) -> usize {
    state.unread + updates.len()
}

impl State {
    /// The inbox's element under `title`, while it has unread mail, which
    /// it keeps borrowed as long as the inbox.
    pub fn titled<'a>(&'a self, title: &'a str) -> ui::Element<'a, Message> {
        let label = if self.unread > 0 {
            title
        } else {
            &self.greeting
        };
        ui::button(label, Message::Read(self.unread))
    }
}
