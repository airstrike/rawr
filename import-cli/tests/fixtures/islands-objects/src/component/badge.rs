// rawr
//! A leaf of an existing app, marked to hot-reload: its page is a marked
//! type that implements the native `Page` trait, and native code keeps it
//! as a `Box<dyn page::Page>`.

use crate::page::{self, Message, Task};

pub struct BadgePage {
    page_index: usize,
    count: usize,
    label: String,
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
                self.label = label;
                Task::Log(log)
            }
        }
    }

    fn view(&self) -> String {
        format!("{} {}", self.label, self.count)
    }
}

pub fn new(page_index: usize) -> Box<dyn page::Page> {
    Box::new(BadgePage {
        page_index,
        count: 0,
        label: "clicks".to_string(),
    })
}
