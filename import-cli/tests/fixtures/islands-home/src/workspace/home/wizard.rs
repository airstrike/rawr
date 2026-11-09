//! Native: a child of the marked `home`, whose tasks and elements `home`
//! maps into its own messages, and whose state, which can't be cloned,
//! `home` keeps in an `Option` while it is open.

use ui::common;

use super::super::{inbox, thumbs};

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Step(u32),
}

/// What the wizard reads, as inboard's `wizard::source::View`: a `Copy`
/// value of borrows, which the marked `home` takes by value and gives on.
#[derive(Debug, Clone, Copy)]
pub struct View<'a> {
    inbox: &'a inbox::State,
}

impl<'a> View<'a> {
    pub fn new(inbox: &'a inbox::State) -> Self {
        Self { inbox }
    }

    pub fn unread(&self) -> usize {
        self.inbox.unread
    }

    pub fn greeting(&self) -> &'a str {
        self.inbox.greeting()
    }
}

pub fn start(count: u32, catalog: View<'_>) -> ui::Task<Message> {
    println!("started over {} unread", catalog.unread());
    ui::Task::done(Message::Step(count))
}

/// The wizard's element, which shows `label` and borrows it.
pub fn view(label: &str, count: u32) -> ui::Element<'_, Message> {
    ui::button(label, Message::Step(count))
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

    /// The open wizard's element over `catalog`, as inboard's
    /// `wizard.view(catalog, theme)`: it borrows what the catalog borrows,
    /// or the theme's name past step 5.
    pub fn view<'a>(
        &'a self,
        catalog: View<'a>,
        theme: &'a crate::Theme,
    ) -> ui::Element<'a, Message> {
        println!("wizard view at {}", self.step);
        let label = if self.step > 5 {
            theme.name()
        } else {
            catalog.greeting()
        };
        ui::button(label, Message::Step(self.step))
    }

    /// Take `message`, as inboard's `wizard.update(message, catalog,
    /// thumbs)`: each step draws a thumbnail and asks for a step of the
    /// catalog's unread count. A step of none closes the wizard, and one
    /// past 9 commits it.
    pub fn update(
        &mut self,
        message: Message,
        catalog: View<'_>,
        thumbs: &mut thumbs::Renderer,
    ) -> common::Action<Instruction, Message> {
        let Message::Step(step) = message;
        self.step += step;
        thumbs.drawn += 1;
        if step == 0 {
            common::Action::new(Instruction::Closed, ui::Task::none())
        } else if self.step > 9 {
            common::Action::new(Instruction::Commit(self.step), ui::Task::none())
        } else {
            let unread = u32::try_from(catalog.unread()).unwrap_or(u32::MAX);
            common::Action::task(ui::Task::done(Message::Step(unread)))
        }
    }
}

/// What the wizard asks of the home screen, as inboard's
/// `wizard::Instruction`.
#[derive(Debug)]
pub enum Instruction {
    Closed,
    Commit(u32),
}

impl Drop for State {
    fn drop(&mut self) {
        println!("wizard closed at step {}", self.step);
    }
}
