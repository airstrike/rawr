// rawr
//! A marked page that implements the native `Page` trait but not its
//! supertrait `Debug`.

use crate::page::Page;

pub struct BadgePage {
    count: usize,
}

impl Page for BadgePage {
    fn title(&self) -> String {
        format!("badge {}", self.count)
    }
}

pub fn new() -> Box<dyn Page> {
    Box::new(BadgePage { count: 1 })
}
