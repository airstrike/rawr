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

pub const SCALE: usize = 10;

pub fn scale(by: usize) -> usize {
    by * SCALE
}

pub mod size {
    pub type Width = usize;

    pub const BASE: usize = 1;
}
