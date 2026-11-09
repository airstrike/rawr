//! Native: the tile wizard, a child of the marked `home`, as inboard's
//! `home::wizard`: `editing` gives the open wizard and its first task as a
//! tuple, and its instructions carry boxed values, one in a struct
//! variant, which `home` moves into its own.

use ui::common;

pub mod commit {
    /// Where a committed tile lands.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub enum Target {
        New,
        Replace(usize),
    }
}

/// A tile, as inboard's `Infolet`: it can't be cloned.
#[derive(Debug)]
pub struct Infolet {
    pub title: String,
}

impl Infolet {
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_owned(),
        }
    }
}

/// A source to provision, as inboard's `provision::Request`.
#[derive(Debug)]
pub struct Request {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Step(u32),
}

/// What the wizard reads: a `Copy` value of borrows.
#[derive(Debug, Clone, Copy)]
pub struct View<'a> {
    sources: &'a [String],
}

impl<'a> View<'a> {
    pub fn new(sources: &'a [String]) -> Self {
        Self { sources }
    }
}

#[derive(Debug)]
pub enum Instruction {
    Closed,
    Commit {
        target: commit::Target,
        infolet: Box<Infolet>,
    },
    Provision(Box<Request>),
}

#[derive(Debug)]
pub struct State {
    infolet: Option<Infolet>,
    step: u32,
    sources: usize,
}

impl State {
    pub fn new() -> Self {
        Self {
            infolet: None,
            step: 0,
            sources: 0,
        }
    }

    /// The wizard editing `infolet`, and its first step.
    pub fn editing(infolet: Infolet, catalog: View<'_>) -> (Self, ui::Task<Message>) {
        let state = Self {
            infolet: Some(infolet),
            step: 0,
            sources: catalog.sources.len(),
        };
        (state, ui::Task::done(Message::Step(1)))
    }

    /// Each step asks for the next: the first provisions a source, the
    /// second commits the tile in place, and one past it closes.
    pub fn update(&mut self, message: Message) -> common::Action<Instruction, Message> {
        let Message::Step(step) = message;
        self.step += step;
        match self.step {
            1 => common::Action::new(
                Instruction::Provision(Box::new(Request {
                    name: format!("source {}", self.sources),
                })),
                ui::Task::done(Message::Step(1)),
            ),
            2 => match self.infolet.take() {
                Some(infolet) => common::Action::new(
                    Instruction::Commit {
                        target: commit::Target::Replace(self.sources),
                        infolet: Box::new(infolet),
                    },
                    ui::Task::none(),
                ),
                None => common::Action::new(Instruction::Closed, ui::Task::none()),
            },
            _ => common::Action::new(Instruction::Closed, ui::Task::none()),
        }
    }
}
