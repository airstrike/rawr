// rawr
//! The home screen, marked, as inboard's `workspace/screen/home.rs`: the
//! native workspace keeps its `State`, whose editor content can't be
//! cloned, between calls, and its `update` gives the crate's tasks over
//! its own `Message`, mapping its native child's with a variant. It keeps
//! its open wizard in an `Option`, its tabs in a `Vec` and its panes, a
//! marked type over a tab, in a `Vec`, none of which can be cloned.

use ui::{common, opaque, stack, Action, Content, Element, Task, Widget};

use super::{inbox, thumbs};

pub mod tab;
pub mod wizard;

const STEP: u32 = 1;

#[derive(Debug, Clone)]
pub enum Message {
    Edited(Action),
    Inbox(inbox::Message),
    Wizard(wizard::Message),
    Built(wizard::Message),
    Count,
}

/// What the screen asks of the workspace, as inboard's `Instruction`.
#[derive(Debug)]
pub enum Instruction {
    Commit(u32),
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
    /// ..)`, shows its child's text. It takes a native grid element by value
    /// inside an `Option` and the inbox's updates as a lent slice, which
    /// the free `inbox::view` keeps, and shows its open wizard, a native
    /// part of `self`, over the catalog and the theme it is given, stacked
    /// over the page with an `opaque` layer, as inboard's does.
    pub fn view<'a>(
        &'a self,
        inbox: &'a inbox::State,
        updates: &'a [inbox::Update],
        catalog: wizard::View<'a>,
        grid: Option<Element<'a, wizard::Message>>,
        theme: &'a crate::Theme,
    ) -> Element<'a, Message> {
        if let Some(grid) = grid {
            return grid.map(Message::Wizard);
        }
        let page: Element<'a, Message> = if inbox.unread == 0 {
            inbox::view(inbox, updates).map(Message::Inbox)
        } else if self.count > 3 {
            ui::section(None, [ui::button(&self.title, Message::Count)])
        } else if inbox.unread > 3 {
            inbox.view().map(Message::Inbox)
        } else {
            wizard::view(&self.title, self.count).map(Message::Wizard)
        };
        match &self.wizard {
            Some(wizard) => stack![
                page,
                opaque(wizard.view(catalog, theme).map(Message::Wizard))
            ]
            .boxed(),
            None => page,
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

/// Take `message`, as inboard's `update`: its open wizard, a native part
/// of `state`, takes its own over the catalog and the thumbnail renderer
/// it is lent, and says whether it closed or committed.
pub fn update(
    state: &mut State,
    message: Message,
    inbox: &mut inbox::State,
    catalog: wizard::View<'_>,
    thumbs: &mut thumbs::Renderer,
) -> common::Action<Instruction, Message> {
    match message {
        Message::Edited(action) => {
            state.content.perform(action);
            common::Action::none()
        }
        Message::Inbox(message) => common::Action::task(inbox.update(message).map(Message::Inbox)),
        Message::Wizard(wizard::Message::Step(step)) if step > 2 => {
            common::Action::task(Task::done(Message::Count))
        }
        Message::Wizard(wizard::Message::Step(step)) => {
            state.count += step;
            common::Action::none()
        }
        Message::Built(message) => {
            let Some(wizard) = state.wizard.as_mut() else {
                return common::Action::none();
            };
            let action = wizard.update(message, catalog, thumbs);
            let task = action.task.map(Message::Built);
            match action.instruction {
                Some(wizard::Instruction::Closed) => {
                    state.wizard = None;
                    common::Action::task(task)
                }
                Some(wizard::Instruction::Commit(step)) => {
                    state.wizard = None;
                    common::Action::new(Instruction::Commit(step), task)
                }
                None => common::Action::task(task),
            }
        }
        Message::Count => {
            state.count += STEP;
            common::Action::task(Task::done(Message::Edited(Action::Insert('!'))))
        }
    }
}

/// A banner over the screen's title, headed by a literal of this file: native
/// code keeps it and draws it across a save, as iced keeps a view's
/// elements until the next message. The literal is longer than the
/// strings rawr interns, so only the generation that read it holds it.
pub fn banner(state: &State) -> Element<'_, Message> {
    ui::section(
        Some("banner one, a heading longer than an interned string"),
        [ui::button(&state.title, Message::Count)],
    )
}

/// The screen's button: a section headed by its text past a count of 3,
/// under a header row of no actions, whose elements it takes by value as
/// inboard's `sidebar::header_row("Home", std::iter::empty())` does.
pub fn view(state: &State) -> Element<'_, Message> {
    if state.count > 3 {
        let _header = ui::header_row("count", Vec::<Element<'_, Message>>::new());
        ui::section(Some("count"), [ui::button(&state.title, Message::Count)])
    } else if state.count > 1 {
        ui::button(&state.greeting, wizard::Message::Step(state.count)).map(Message::Wizard)
    } else {
        ui::button("wizard", wizard::Message::Step(state.count)).map(Message::Wizard)
    }
}

/// A banner over the inbox alone, as inboard's `threads::view(threads:
/// &'a Store)`: its element borrows the program's text and no marked
/// input, so it is anchored on the native one.
pub fn inbox_banner<'a>(inbox: &'a inbox::State) -> Element<'a, Message> {
    ui::button("banner", inbox::Message::Read(inbox.unread)).map(Message::Inbox)
}

/// The inbox's top bar over its updates, which the free `inbox::view`
/// keeps, with no marked input to anchor on.
pub fn top_bar<'a>(inbox: &'a inbox::State, updates: &'a [inbox::Update]) -> Element<'a, Message> {
    inbox::view(inbox, updates).map(Message::Inbox)
}

/// The top bar's text, read where it is made: its element keeps the lent
/// inbox and updates for this call alone.
pub fn top_label(inbox: &inbox::State, updates: &[inbox::Update]) -> String {
    inbox::view(inbox, updates).label.to_owned()
}

/// How many wait, through a crate fn that keeps neither the lent inbox nor
/// the lent updates.
pub fn pending(inbox: &inbox::State, updates: &[inbox::Update]) -> usize {
    inbox::pending(inbox, updates)
}

impl State {
    /// The inbox's element under the screen's title, through a crate
    /// method that keeps the `&'a str` as long as the inbox.
    pub fn titled<'a>(&'a self, inbox: &'a inbox::State) -> Element<'a, Message> {
        inbox.titled(&self.title).map(Message::Inbox)
    }
}

/// The inbox's element under `title`, its text read where it is made.
pub fn titled_label(inbox: &inbox::State, title: &str) -> String {
    inbox.titled(title).label.to_owned()
}

/// The open wizard's label over the theme, read where it is made: the
/// wizard's `view` keeps the theme, lent for this call alone.
pub fn wizard_label(state: &State, catalog: wizard::View<'_>, theme: &crate::Theme) -> String {
    match &state.wizard {
        Some(wizard) => wizard.view(catalog, theme).label.to_owned(),
        None => "closed".to_owned(),
    }
}

/// A notification's id, as inboard's `inbox::Id`, which native code makes
/// from any text.
#[derive(Debug, Clone, PartialEq)]
pub struct Id(String);

impl Id {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

/// The ids read, as inboard's `inbox::State`, which native code marks read
/// from any iterable of ids, one that borrows its locals too.
#[derive(Debug, Default)]
pub struct Read {
    ids: Vec<Id>,
}

impl Read {
    pub fn mark_all_read(&mut self, ids: impl IntoIterator<Item = Id>) {
        self.ids.extend(ids);
    }

    pub fn label(&self) -> String {
        let ids = self.ids.iter().map(|id| id.0.as_str()).collect::<Vec<_>>();
        ids.join(" ")
    }
}
