// rawr
//! A leaf of an existing app, marked to hot-reload: its page is a marked
//! type that implements the native `Page` trait, and native code keeps it
//! as a `Box<dyn page::Page>`.

use std::rc::Rc;
use std::sync::Arc;

use crate::page::{self, Message, Task};

#[derive(Clone, Debug)]
pub struct BadgePage {
    page_index: usize,
    count: usize,
    label: String,
    names: Vec<String>,
}

impl page::Page for BadgePage {
    fn title(&self) -> String {
        format!("badge {}", self.page_index)
    }

    fn update(&mut self, message: Message) -> Task {
        match message {
            Message::Bump(by) => {
                self.count += by;
                Task::None
            }
            Message::Rename(label) => {
                let log = format!("renamed {} to {label}", self.label);
                let old = std::mem::replace(&mut self.label, label);
                self.names.push(old);
                Task::Log(log)
            }
        }
    }

    fn view(&self) -> String {
        format!("{} {}", self.label, self.count)
    }

    // The shown page at `index` may be this one: its `title` runs while
    // this call does, sharing the page.
    fn summary(&self, index: usize) -> String {
        format!("{} sees {}", self.view(), page::title_of(index))
    }

    // Native code holds these borrows of the page after the call returns.
    fn label(&self) -> &str {
        &self.label
    }

    fn items(&self) -> &[String] {
        &self.names
    }

    // What it draws borrows the page's own label.
    fn draw(&self) -> page::Drawn<'_> {
        page::drawn(&self.label)
    }
}

pub fn new(page_index: usize) -> Box<dyn page::Page> {
    Box::new(BadgePage {
        page_index,
        count: 0,
        label: "clicks".to_string(),
        names: Vec::new(),
    })
}

/// A page lent to native code as a `&dyn page::Page`, coerced and as
/// rustc coerces it at the call, and given as an `Rc` and an `Arc` of
/// one, while marked code keeps it (#1304).
pub fn described(page_index: usize) -> String {
    let page = BadgePage {
        page_index,
        count: 2,
        label: "lent".to_string(),
        names: Vec::new(),
    };
    let first = page::describe(&page as &dyn page::Page);
    let second = page::describe(&page);
    let counted = page::counted(Rc::new(page.clone()) as Rc<dyn page::Page>);
    let shared: Arc<dyn page::Page> = Arc::new(page.clone());
    let shared = page::shared(shared);
    format!(
        "{first}; {second}; {counted}; {shared}; still {}",
        page.count
    )
}

/// A page of a private type native code keeps while the program runs, as
/// a `Box<dyn page::Page>`: the running program holds its value, and its
/// step's, which native code never names (#1746).
#[derive(Clone, Debug)]
struct Spare {
    ticks: usize,
    step: Step,
}

/// How far a spare page counts at each update.
#[derive(Clone, Debug)]
struct Step {
    by: usize,
}

impl page::Page for Spare {
    fn title(&self) -> String {
        "spare".to_string()
    }

    fn update(&mut self, _message: Message) -> Task {
        self.ticks += self.step.by;
        Task::None
    }

    fn view(&self) -> String {
        format!("spare {}", self.ticks)
    }

    fn summary(&self, _index: usize) -> String {
        self.view()
    }

    fn label(&self) -> &str {
        "spare"
    }

    fn items(&self) -> &[String] {
        &[]
    }

    fn draw(&self) -> page::Drawn<'_> {
        page::drawn("spare")
    }
}

/// The page native code keeps while the program runs.
pub fn spare() -> Box<dyn page::Page> {
    Box::new(Spare {
        ticks: 0,
        step: Step { by: 1 },
    })
}
