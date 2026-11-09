//! A stand-in for iced: a task that runs its work when it is polled, a
//! text editor's content, which can't be cloned, and an element generic
//! over the app's message.

use std::rc::Rc;

/// Work that gives values of `T` when it is polled, as `iced::Task`: a
/// mapped task calls its function only then.
pub struct Task<T> {
    work: Vec<Box<dyn FnOnce() -> T>>,
}

impl<T: 'static> Task<T> {
    pub fn none() -> Self {
        Self { work: Vec::new() }
    }

    pub fn done(value: T) -> Self {
        Self {
            work: vec![Box::new(move || value)],
        }
    }

    pub fn map<U: 'static>(self, f: impl Fn(T) -> U + 'static) -> Task<U> {
        let f = Rc::new(f);
        Task {
            work: self
                .work
                .into_iter()
                .map(|work| {
                    let f = Rc::clone(&f);
                    Box::new(move || f(work())) as Box<dyn FnOnce() -> U>
                })
                .collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.work.len()
    }

    /// Run the work, as iced's runtime does.
    pub fn poll(self) -> Vec<T> {
        self.work.into_iter().map(|work| work()).collect()
    }
}

/// What an edit does, as `text_editor::Action`.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Insert(char),
    Clear,
}

/// A text editor's content, as `text_editor::Content`: it can't be cloned.
#[derive(Debug, Default)]
pub struct Content {
    text: String,
    edits: usize,
}

impl Content {
    pub fn perform(&mut self, action: Action) {
        self.edits += 1;
        match action {
            Action::Insert(character) => self.text.push(character),
            Action::Clear => self.text.clear(),
        }
    }

    pub fn text(&self) -> String {
        self.text.clone()
    }

    pub fn edits(&self) -> usize {
        self.edits
    }
}

/// A view of the state, as `iced::Element<'a, Message>`, which an app maps
/// into its own messages: it borrows the text it shows for `'a`.
pub struct Element<'a, M> {
    pub label: &'a str,
    pub on_press: Option<M>,
}

pub fn button<'a, M>(label: &'a str, on_press: M) -> Element<'a, M> {
    Element {
        label,
        on_press: Some(on_press),
    }
}

impl<'a, M> Element<'a, M> {
    /// The element over `N`, `f` called at once on its message, as
    /// iced's `Element::map` wraps each message its widgets give.
    pub fn map<N>(self, f: impl Fn(M) -> N) -> Element<'a, N> {
        Element {
            label: self.label,
            on_press: self.on_press.map(f),
        }
    }
}
