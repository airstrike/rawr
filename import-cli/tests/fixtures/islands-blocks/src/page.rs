//! Native: what a page is made of, as a component writes it.

/// One block of a page: prose, an example or a group of blocks.
#[derive(Debug)]
pub enum Block {
    Md(&'static str),
    Example {
        id: &'static str,
        title: &'static str,
        description: Option<&'static str>,
        snippet: &'static str,
    },
    Group {
        title: &'static str,
        children: &'static [Block],
    },
}

/// Lays `blocks` out, one line a block, indented by `depth`.
pub fn lay_out(blocks: &[Block], depth: usize) {
    for block in blocks {
        let indent = "  ".repeat(depth);
        match block {
            Block::Md(text) => println!("{indent}md {text}"),
            Block::Example {
                id,
                title,
                description,
                snippet,
            } => println!("{indent}example {id} {title:?} {description:?} {snippet:?}"),
            Block::Group { title, children } => {
                println!("{indent}group {title}");
                lay_out(children, depth + 1);
            }
        }
    }
}

/// What a block is called: an example's title, a group's, or its text.
pub fn heading(block: Block) -> String {
    match block {
        Block::Md(text) => text.to_owned(),
        Block::Example { title, .. } | Block::Group { title, .. } => title.to_owned(),
    }
}
