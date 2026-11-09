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

/// A view of the state, as `iced::Element`, which an app maps into its
/// own messages.
pub struct Element<M> {
    pub label: String,
    pub on_press: Option<M>,
}

pub fn button<M>(label: &str, on_press: M) -> Element<M> {
    Element {
        label: label.to_owned(),
        on_press: Some(on_press),
    }
}

impl<M> Element<M> {
    pub fn map<N>(self, f: impl Fn(M) -> N) -> Element<N> {
        Element {
            label: self.label,
            on_press: self.on_press.map(f),
        }
    }
}
