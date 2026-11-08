//! Native: the app's pages, behind a trait, and what they take and give.

pub trait Page: Send {
    fn title(&self) -> String;
    fn update(&mut self, message: Message) -> Task;
    fn view(&self) -> String;
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
}
