//! Native: the app's pages, behind a trait.

pub trait Page: Send {
    fn title(&self) -> String;
}

pub struct Home;

impl Page for Home {
    fn title(&self) -> String {
        "home".to_string()
    }
}

pub struct About;

impl Page for About {
    fn title(&self) -> String {
        "about".to_string()
    }
}

/// An alias of a native type: it crosses as the native type.
pub type Landing = Home;

pub const SCALE: usize = 10;

pub fn scale(by: usize) -> usize {
    by * SCALE
}

pub mod size {
    pub type Width = usize;

    pub const BASE: usize = 1;
}

/// A native struct a marked function changes through a `&mut` it is lent.
pub struct Tally {
    count: usize,
}

impl Tally {
    pub fn new() -> Self {
        Tally { count: 0 }
    }

    pub fn add(&mut self, by: usize) {
        self.count += by;
    }

    pub fn count(&self) -> usize {
        self.count
    }
}
