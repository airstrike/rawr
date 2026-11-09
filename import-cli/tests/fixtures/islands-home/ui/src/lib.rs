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

/// A section of `children` under `heading`, as inboard's
/// `sidebar::nav_section`: its element borrows the heading, or the first
/// child's label when it has none.
pub fn section<'a, M: 'a>(
    heading: Option<&'a str>,
    children: impl IntoIterator<Item = Element<'a, M>>,
) -> Element<'a, M> {
    let first = children.into_iter().next();
    Element {
        label: heading
            .or(first.as_ref().map(|child| child.label))
            .unwrap_or(""),
        on_press: first.and_then(|child| child.on_press),
    }
}

/// A widget, as `iced::Widget`: it boxes into an element, which is one
/// too.
pub trait Widget<M> {
    /// The widget as an element, as iced's `Widget::_boxed`, which its
    /// `stack!` calls on each child.
    fn _boxed<'b>(self) -> Element<'b, M>
    where
        Self: Sized + 'b;

    /// The widget as an element, as iced's `Widget::boxed`.
    fn boxed<'b>(self) -> Element<'b, M>
    where
        Self: Sized + 'b,
    {
        self._boxed()
    }
}

/// An element boxes as itself, and says so, so the order its boxes run
/// in shows.
impl<'a, M> Widget<M> for Element<'a, M> {
    fn _boxed<'b>(self) -> Element<'b, M>
    where
        Self: Sized + 'b,
    {
        println!("boxed {}", self.label);
        self
    }
}

/// Elements layered over each other, as `iced::widget::Stack`: the last
/// is on top.
pub struct Stack<'a, M> {
    children: Vec<Element<'a, M>>,
}

impl<'a, M> Stack<'a, M> {
    pub fn with_children(children: impl IntoIterator<Item = Element<'a, M>>) -> Self {
        Self {
            children: children.into_iter().collect(),
        }
    }
}

/// The stack shows its top child, and says how many it layers.
impl<'a, M> Widget<M> for Stack<'a, M> {
    fn _boxed<'b>(self) -> Element<'b, M>
    where
        Self: Sized + 'b,
    {
        println!("stacked {}", self.children.len());
        self.children.into_iter().last().unwrap_or(Element {
            label: "",
            on_press: None,
        })
    }
}

/// `content`, taking every event over it, as `iced::widget::opaque`.
pub fn opaque<'a, M: 'a>(content: impl Widget<M> + 'a) -> impl Widget<M> + 'a {
    content._boxed()
}

/// Elements layered over each other, as the `stack!` of inboard's iced,
/// which boxes each child with `Widget::_boxed`.
#[macro_export]
macro_rules! stack {
    ($($x:expr),+ $(,)?) => (
        $crate::Stack::with_children([$($crate::Widget::_boxed($x)),+])
    );
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

pub mod common {
    //! What a screen's `update` gives back, as inboard's `common::Action`:
    //! an instruction for its parent, if any, and a task over its messages.

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

        pub fn task(task: Task<M>) -> Self {
            Self {
                instruction: None,
                task,
            }
        }
    }
}
