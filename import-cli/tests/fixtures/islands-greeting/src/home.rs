// rawr
//! The home screen, marked, as inboard's `workspace/screen/home.rs`: a
//! greeting picked by a crate's generator and clock through their traits,
//! a recent list sorted newest first and shown as right-click drop-downs,
//! and an `update` that moves its native wizard's boxed values into its
//! own instructions.

use clock::Timelike;
use dice::Rng;
use ui::common::Action;
use ui::{Element, Widget};

use crate::registry::{Entry, Id, Registry};

pub mod wizard;

/// How many recent entries the sidebar lists.
const MAX_RECENT: usize = 2;

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum Message {
    InputChanged(ui::Edit),
    InputResized(u32),
    FocusInput(Option<u32>),
    Submit,
    OpenProject(Id),
    LeaveProject(Id),
    Wizard(wizard::Message),
    NoOp,
}

#[derive(Debug)]
pub enum Instruction {
    Commit {
        target: wizard::commit::Target,
        infolet: Box<wizard::Infolet>,
    },
    Provision(Box<wizard::Request>),
    OpenProject(Id),
    LeaveProject(Id),
}

#[derive(Debug)]
pub struct State {
    input_value: ui::Content,
    greeting_index: usize,
    wizard: Option<wizard::State>,
}

impl State {
    pub fn new() -> Self {
        Self {
            input_value: ui::Content::default(),
            greeting_index: dice::rng().random_range(0..100),
            wizard: None,
        }
    }

    /// Open the wizard editing `infolet`, and give its first task.
    pub fn edit_tile(
        &mut self,
        infolet: wizard::Infolet,
        catalog: wizard::View<'_>,
    ) -> ui::Task<Message> {
        let (wizard, task) = wizard::State::editing(infolet, catalog);
        self.wizard = Some(wizard);
        task.map(Message::Wizard)
    }

    pub fn open_wizard(&mut self) {
        self.wizard = Some(wizard::State::new());
    }

    pub fn greeting(&self, name: &str) -> String {
        greeting(self.greeting_index, name)
    }

    pub fn update(&mut self, message: Message) -> Action<Instruction, Message> {
        match message {
            Message::InputChanged(edit) => {
                self.input_value.perform(edit);
                Action::none()
            }
            Message::InputResized(_) | Message::FocusInput(_) => Action::none(),
            Message::Submit => {
                let text = self.input_value.text().trim().to_string();
                if text.is_empty() {
                    return Action::none();
                }
                println!("submitted {text}");
                self.input_value = ui::Content::default();
                Action::none()
            }
            Message::OpenProject(id) => Action::instruction(Instruction::OpenProject(id)),
            Message::LeaveProject(id) => Action::instruction(Instruction::LeaveProject(id)),
            Message::NoOp => Action::none(),
            Message::Wizard(message) => {
                let Some(wizard) = self.wizard.as_mut() else {
                    return Action::none();
                };
                let action = wizard.update(message);
                let task = action.task.map(Message::Wizard);
                match action.instruction {
                    Some(wizard::Instruction::Closed) => {
                        self.wizard = None;
                        Action::task(task)
                    }
                    Some(wizard::Instruction::Commit { target, infolet }) => {
                        self.wizard = None;
                        Action::new(Instruction::Commit { target, infolet }, task)
                    }
                    Some(wizard::Instruction::Provision(request)) => {
                        Action::new(Instruction::Provision(request), task)
                    }
                    None => Action::task(task),
                }
            }
        }
    }

    pub(crate) fn sidebar<'a>(&'a self, registry: &'a Registry) -> Element<'a, Message> {
        let header = ui::section(Some("Home"), std::iter::empty());
        ui::section(None, [header, build_recent_section(registry)])
    }
}

/// A greeting from the pool for the clock's hour.
fn greeting(index: usize, name: &str) -> String {
    let hour = clock::Local::now().hour();
    let greetings: &[&str] = match hour {
        5..12 => &["Good morning, {name}.", "Fresh day ahead."],
        12..17 => &[
            "What are we tackling next?",
            "Afternoon, {name}. How can I help?",
        ],
        _ => &["The night is young.", "Still going, {name}?"],
    };
    greetings[index % greetings.len()].replace("{name}", name)
}

/// The recent entries, newest first, each a drop-down that opens on a
/// right click.
fn build_recent_section<'a>(registry: &'a Registry) -> Element<'a, Message> {
    use std::cmp::Reverse;

    let mut entries: Vec<&Entry> = registry
        .entries()
        .iter()
        .filter(|e| !e.is_trashed())
        .collect();
    entries.sort_by_key(|e| Reverse(e.opened_at()));
    entries.truncate(MAX_RECENT);

    if entries.is_empty() {
        return ui::section(Some("QUICK ACCESS"), [ui::empty("No recent files")]);
    }

    let rows: Vec<Element<'a, Message>> = entries
        .into_iter()
        .map(|entry| {
            let base = ui::item(
                entry.name(),
                entry.opened_at().humanize(),
                Message::OpenProject(entry.id()),
            );
            let menu = recent_context_menu(entry);
            ui::drop_down(base, menu).right_click().boxed()
        })
        .collect();

    ui::section(Some("QUICK ACCESS"), rows)
}

fn recent_context_menu<'a>(entry: &'a Entry) -> Element<'a, Message> {
    let id = entry.id();
    if entry.is_shared() {
        ui::button("Leave", Message::LeaveProject(id))
    } else {
        ui::button("Open", Message::OpenProject(id))
    }
}
