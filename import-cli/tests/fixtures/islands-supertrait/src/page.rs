//! Native: the app's pages, behind a trait that needs `Debug`.

use std::fmt::Debug;

pub trait Page: Debug {
    fn title(&self) -> String;
}
