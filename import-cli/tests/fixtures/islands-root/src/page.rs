//! Native: a page's own messages.

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Noop,
    Open(usize),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Narrow,
    Wide,
}
