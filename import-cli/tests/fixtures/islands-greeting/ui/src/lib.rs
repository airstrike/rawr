//! A stand-in for iced and inboard's `common`: a task that runs its work
//! when it is polled, an element over the app's message, a `Widget` trait
//! whose `boxed` makes an element, a drop-down built by value, and the
//! `Action` a screen's `update` gives back.

use std::marker::PhantomData;
use std::rc::Rc;

/// Work that gives values of `T` when it is polled, as `iced::Task`.
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
pub enum Edit {
    Insert(char),
}

/// A text editor's content, as `text_editor::Content`: it can't be cloned.
#[derive(Debug, Default)]
pub struct Content {
    text: String,
}

impl Content {
    pub fn perform(&mut self, edit: Edit) {
        let Edit::Insert(character) = edit;
        self.text.push(character);
    }

    pub fn text(&self) -> String {
        self.text.clone()
    }
}

/// A view, as `iced::Element<'a, Message>`: its text and the messages its
/// presses give.
pub struct Element<'a, M> {
    text: String,
    presses: Vec<M>,
    borrows: PhantomData<&'a str>,
}

impl<'a, M: std::fmt::Debug> Element<'a, M> {
    /// What it shows and gives, for native code to print.
    pub fn describe(&self) -> String {
        format!("{} {:?}", self.text, self.presses)
    }
}

/// What makes an element, as inboard's `Widget::boxed`.
pub trait Widget<'a, M> {
    fn boxed(self) -> Element<'a, M>;
}

impl<'a, M> Widget<'a, M> for Element<'a, M> {
    fn boxed(self) -> Element<'a, M> {
        self
    }
}

pub fn button<'a, M>(label: &'a str, on_press: M) -> Element<'a, M> {
    Element {
        text: label.to_owned(),
        presses: vec![on_press],
        borrows: PhantomData,
    }
}

/// A row with a label and its meta text, as inboard's
/// `sidebar::nav_item_with_meta`.
pub fn item<'a, M>(label: &'a str, meta: String, on_press: M) -> Element<'a, M> {
    Element {
        text: format!("{label} ({meta})"),
        presses: vec![on_press],
        borrows: PhantomData,
    }
}

pub fn empty<'a, M>(label: &'a str) -> Element<'a, M> {
    Element {
        text: label.to_owned(),
        presses: Vec::new(),
        borrows: PhantomData,
    }
}

/// `children` under `heading`, as inboard's `sidebar::nav_section`.
pub fn section<'a, M>(
    heading: Option<&'a str>,
    children: impl IntoIterator<Item = Element<'a, M>>,
) -> Element<'a, M> {
    let mut text = heading.unwrap_or("").to_owned();
    let mut presses = Vec::new();
    for child in children {
        text.push_str(" [");
        text.push_str(&child.text);
        text.push(']');
        presses.extend(child.presses);
    }
    Element {
        text,
        presses,
        borrows: PhantomData,
    }
}

/// A base that opens a menu, as inboard's `drop_down`: built by value.
pub struct DropDown<'a, M> {
    base: Element<'a, M>,
    menu: Element<'a, M>,
    button: &'static str,
}

pub fn drop_down<'a, M: 'a>(
    base: impl Widget<'a, M> + 'a,
    menu: impl Widget<'a, M> + 'a,
) -> DropDown<'a, M> {
    DropDown {
        base: base.boxed(),
        menu: menu.boxed(),
        button: "left",
    }
}

impl<'a, M> DropDown<'a, M> {
    pub fn right_click(mut self) -> Self {
        self.button = "right";
        self
    }
}

impl<'a, M> Widget<'a, M> for DropDown<'a, M> {
    fn boxed(self) -> Element<'a, M> {
        let mut presses = self.base.presses;
        presses.extend(self.menu.presses);
        Element {
            text: format!(
                "{} on {} opens {}",
                self.base.text, self.button, self.menu.text
            ),
            presses,
            borrows: PhantomData,
        }
    }
}

pub mod common {
    //! What a screen's `update` gives back, as inboard's `common::Action`.

    use crate::Task;

    pub struct Action<I, M> {
        pub instruction: Option<I>,
        pub task: Task<M>,
    }

    impl<I, M: 'static> Action<I, M> {
        pub fn none() -> Self {
            Self {
                instruction: None,
                task: Task::none(),
            }
        }

        pub fn new(instruction: I, task: Task<M>) -> Self {
            Self {
                instruction: Some(instruction),
                task,
            }
        }

        pub fn instruction(instruction: impl Into<I>) -> Self {
            Self {
                instruction: Some(instruction.into()),
                task: Task::none(),
            }
        }

        pub fn task(task: Task<M>) -> Self {
            Self {
                instruction: None,
                task,
            }
        }
    }
}
