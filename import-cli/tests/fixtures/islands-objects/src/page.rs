//! Native: the app's pages, behind a trait, and what they take and give.

use std::cell::RefCell;
use std::fmt::Debug;

pub trait Page: Debug + PageClone + Send {
    fn title(&self) -> String;
    fn update(&mut self, message: Message) -> Task;
    fn view(&self) -> String;
    /// What the page says of the shown page at `index`, which it may be.
    fn summary(&self, index: usize) -> String;
    /// What the page is called, borrowed from it.
    fn label(&self) -> &str;
    /// The names the page had, borrowed from it.
    fn items(&self) -> &[String];
    /// What the page draws, which borrows it, as a view's element does.
    fn draw(&self) -> Drawn<'_>;
}

/// What a page draws: text it borrows, as iced's `Element<'_, Message>`
/// borrows its state.
#[derive(Debug)]
pub struct Drawn<'a> {
    text: &'a str,
}

impl<'a> Drawn<'a> {
    pub fn text(&self) -> &'a str {
        self.text
    }
}

/// `text`, drawn.
pub fn drawn(text: &str) -> Drawn<'_> {
    Drawn { text }
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

/// What a page lent as a trait object says of itself.
pub fn describe(page: &dyn Page) -> String {
    format!("{} is {}", page.title(), page.view())
}

/// What a page shared by count says of itself.
pub fn counted(page: std::rc::Rc<dyn Page>) -> String {
    format!(
        "{} counted {}",
        page.title(),
        std::rc::Rc::strong_count(&page)
    )
}

/// What a page shared across threads says of itself.
pub fn shared(page: std::sync::Arc<dyn Page>) -> String {
    format!("{} shared", page.view())
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

    fn label(&self) -> &str {
        "home"
    }

    fn items(&self) -> &[String] {
        &[]
    }

    fn draw(&self) -> Drawn<'_> {
        drawn("home")
    }
}
