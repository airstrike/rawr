// rawr
//! A leaf of an existing app, marked to hot-reload: its page is native
//! blocks written from literals, and it builds more blocks to hand to
//! native code.

use crate::page;

pub static BLOCKS: &[page::Block] = &[
    page::Block::Example {
        id: "demo",
        title: "",
        description: None,
        snippet: "",
    },
    page::Block::Md("A badge marks a count."),
    page::Block::Group {
        title: "Examples",
        children: &[
            page::Block::Example {
                id: "dot",
                title: "Dot",
                description: Some("A badge with no count."),
                snippet: "badge()",
            },
            page::Block::Md("Counts past 99 show 99+."),
        ],
    },
];

/// A block built from literals, given by value.
pub fn note() -> page::Block {
    page::Block::Group {
        title: "Notes",
        children: &[page::Block::Md("built in a call")],
    }
}

/// Blocks built here, lent to native code.
pub fn show() {
    page::lay_out(&[note(), page::Block::Md("lent to native code")], 1);
}

/// A block built here, given to native code by value.
pub fn heading() -> String {
    page::heading(page::Block::Example {
        id: "count",
        title: "Count",
        description: Some("A badge with a number."),
        snippet: "badge(3)",
    })
}

/// The page as marked code reads it, part of it lent to native code.
pub fn count() -> usize {
    page::lay_out(&BLOCKS[1..2], 1);
    BLOCKS.len()
}
