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

/// The page as marked code walks it: native code's `BLOCKS`, read where it
/// is, a group's children too.
pub fn titles() {
    println!("{} blocks", BLOCKS.len());
    for block in BLOCKS {
        match block {
            page::Block::Md(text) => println!("md {text}"),
            page::Block::Example { id, title, .. } => println!("example {id} {title:?}"),
            page::Block::Group { title, children } => {
                println!("group {title}, {} children", children.len());
                if let page::Block::Example {
                    title, description, ..
                } = &children[0]
                {
                    println!("first {title} {description:?}");
                }
                for child in children.iter() {
                    if let page::Block::Md(text) = child {
                        println!("  md {text}");
                    }
                }
            }
        }
    }
    if let page::Block::Group { children, .. } = &BLOCKS[2] {
        page::lay_out(children, 2);
    }
}

/// The page's state, which keeps native code's `BLOCKS` for as long as the
/// app runs.
pub fn state() -> page::State {
    page::State::new(4, BLOCKS)
}

/// A state that keeps part of `BLOCKS`.
pub fn part() -> page::State {
    page::State::new(5, &BLOCKS[1..2])
}

/// A state that keeps a block written in the call, which Rust promotes to
/// a `'static`.
pub fn promoted() -> page::State {
    page::State::new(6, &[page::Block::Md("promoted")])
}
