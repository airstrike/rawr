// rawr
//! The home screen, marked, as inboard's `workspace/screen/home.rs`: the
//! native workspace keeps its `State`, whose editor content can't be
//! cloned, between calls, and its `update` gives the crate's tasks over
//! its own `Message`, mapping its native child's with a variant. It keeps
//! its open wizard in an `Option`, its tabs in a `Vec` and its panes, a
//! marked type over a tab, in a `Vec`, none of which can be cloned.

use ui::{Action, Content, Element, Task};

use super::inbox;

pub mod tab;
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
    wizard: Option<wizard::State>,
    tabs: Vec<tab::Tab>,
    panes: Vec<Pane>,
    count: u32,
    title: String,
    greeting: String,
}

/// A split of the home screen: a marked type over a native tab, which the
/// state keeps in a `Vec`.
#[derive(Debug)]
pub struct Pane {
    tab: tab::Tab,
    pinned: bool,
}

impl State {
    pub fn new() -> Self {
        Self {
            content: Content::default(),
            wizard: None,
            tabs: Vec::new(),
            panes: Vec::new(),
            count: 0,
            title: "home".to_owned(),
            greeting: "hello".to_owned(),
        }
    }

    pub fn split(&mut self, title: &str) {
        self.panes.push(Pane {
            tab: tab::Tab::new(title),
            pinned: false,
        });
    }

    pub fn pin(&mut self, index: usize) {
        self.panes[index].pinned = true;
    }

    pub fn close_pane(&mut self, index: usize) {
        self.panes.remove(index);
    }

    pub fn panes(&self) -> String {
        let mut panes = Vec::new();
        for pane in &self.panes {
            let pin = if pane.pinned { "*" } else { "" };
            panes.push(format!("{}{pin}", pane.tab.title()));
        }
        panes.join(" | ")
    }

    /// Open a new wizard at the current count, as inboard's
    /// `open_wizard`: one already open closes.
    pub fn open_wizard(&mut self) {
        self.wizard = Some(wizard::State::new(self.count));
    }

    pub fn close_wizard(&mut self) {
        self.wizard = None;
    }

    /// Whether a wizard is open to advance.
    pub fn advance_wizard(&mut self) -> bool {
        match self.wizard.as_mut() {
            Some(wizard) => {
                wizard.advance();
                true
            }
            None => false,
        }
    }

    pub fn push_tab(&mut self, title: &str) {
        self.tabs.push(tab::Tab::new(title));
    }

    pub fn rename_tab(&mut self, index: usize, title: &str) {
        self.tabs[index].rename(title);
    }

    pub fn close_tab(&mut self, index: usize) {
        self.tabs.remove(index);
    }

    pub fn tabs(&self) -> String {
        let titles: Vec<&str> = self.tabs.iter().map(|tab| tab.title()).collect();
        titles.join(", ")
    }

    pub fn layout(&self) -> String {
        match &self.wizard {
            Some(wizard) => format!("wizard at {} over [{}]", wizard.step(), self.tabs()),
            None => format!("no wizard over [{}]", self.tabs()),
        }
    }

    pub fn count(&self) -> u32 {
        self.count
    }

    pub fn text(&self) -> String {
        self.content.text()
    }

    /// Open the wizard at the current count, as inboard's `edit_tile`, over
    /// `catalog`, a native view of borrows it takes by value.
    pub fn open(&mut self, catalog: wizard::View<'_>) -> Task<Message> {
        self.count += STEP;
        println!("catalog of {} unread", catalog.unread());
        wizard::start(self.count, catalog).map(Message::Wizard)
    }

    /// The screen, as inboard's `view<'a>(&'a self, ..)`: an element that
    /// borrows its own text for `'a`, the element of the native inbox it is
    /// lent, which borrows the inbox, or a child's, each mapped into its
    /// messages. A section with no heading, as inboard's `nav_section(None,
    /// ..)`, shows its child's text.
    pub fn view<'a>(&'a self, inbox: &'a inbox::State) -> Element<'a, Message> {
        if self.count > 3 {
            ui::section(None, [ui::button(&self.title, Message::Count)])
        } else if inbox.unread > 3 {
            inbox.view().map(Message::Inbox)
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

/// The screen's button: a section headed by its text past a count of 3.
pub fn view(state: &State) -> Element<'_, Message> {
    if state.count > 3 {
        ui::section(Some("count"), [ui::button(&state.title, Message::Count)])
    } else if state.count > 1 {
        ui::button(&state.greeting, wizard::Message::Step(state.count)).map(Message::Wizard)
    } else {
        ui::button("wizard", wizard::Message::Step(state.count)).map(Message::Wizard)
    }
}
