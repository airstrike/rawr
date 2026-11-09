//! Native: the marked home screen's add-tile wizard, as inboard's
//! `home::wizard`, whose element keeps the catalog's text and the theme
//! borrowed.
use std::cell::Cell;

use crate::theme::Theme;
use crate::Element;

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Step(u32),
}

/// What the wizard reads from, owned by native code.
#[derive(Debug)]
pub struct Source {
    names: Vec<String>,
    reads: Cell<u32>,
}

impl Source {
    pub fn new(names: &[&str]) -> Self {
        Self {
            names: names.iter().map(|name| (*name).to_owned()).collect(),
            reads: Cell::new(0),
        }
    }

    pub fn view(&self) -> View<'_> {
        View { source: self }
    }

    pub fn reads(&self) -> u32 {
        self.reads.get()
    }
}

/// What the wizard reads, as inboard's `wizard::source::View`: a `Copy`
/// value of borrows over a cache it counts reads in, which the marked
/// `home` takes by value for its call and gives on.
#[derive(Debug, Clone, Copy)]
pub struct View<'a> {
    source: &'a Source,
}

impl<'a> View<'a> {
    pub fn first(&self) -> &'a str {
        self.source.reads.set(self.source.reads.get() + 1);
        &self.source.names[0]
    }
}

/// The open wizard: it can't be cloned.
#[derive(Debug)]
pub struct State {
    step: u32,
}

impl State {
    pub fn new(step: u32) -> Self {
        Self { step }
    }

    /// The wizard's element over `catalog` and the theme, as inboard's
    /// `wizard.view(catalog, theme)`.
    pub fn view<'a>(&'a self, catalog: View<'a>, theme: &'a Theme) -> Element<'a, Message> {
        let step = self.step;
        ui::label(catalog.first())
            .style(move |_: &Theme| theme.accent() + step)
            .on_press(Message::Step(step))
    }
}
