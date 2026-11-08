//! Native: the app's pages, behind a trait, and what they take and give.

use std::cell::RefCell;
use std::fmt::Debug;

pub trait Page: Debug + PageClone + Send {
    fn title(&self) -> String;
    fn update(&mut self, message: Message) -> Task;
    fn view(&self) -> String;
    /// What the page says of the shown page at `index`, which it may be.
    fn summary(&self, index: usize) -> String;
}

/// A page's clone as a box, which a `Clone` page has.
pub trait PageClone {
    fn clone_page(&self) -> Box<dyn Page>;
}

impl<T: Page + Clone + 'static> PageClone for T {
    fn clone_page(&self) -> Box<dyn Page> {
        Box::new(self.clone())
    }
}

impl Clone for Box<dyn Page> {
    fn clone(&self) -> Self {
        self.clone_page()
    }
}

thread_local! {
    /// The pages on screen, which a page's own method may read.
    pub static SHOWN: RefCell<Vec<Box<dyn Page>>> = const { RefCell::new(Vec::new()) };
}

/// The title of the shown page at `index`.
pub fn title_of(index: usize) -> String {
    SHOWN.with_borrow(|shown| shown[index].title())
}

/// What a page is told.
#[derive(Debug)]
pub enum Message {
    Bump(usize),
    Rename(String),
}

/// What a page asks for after an update.
#[derive(Debug)]
pub enum Task {
    None,
    Log(String),
}

/// A page native code writes itself.
#[derive(Clone, Debug)]
pub struct Home;

impl Page for Home {
    fn title(&self) -> String {
        "home".to_string()
    }

    fn update(&mut self, _message: Message) -> Task {
        Task::None
    }

    fn view(&self) -> String {
        "welcome".to_string()
    }

    fn summary(&self, index: usize) -> String {
        format!("home beside {}", title_of(index))
    }
}
