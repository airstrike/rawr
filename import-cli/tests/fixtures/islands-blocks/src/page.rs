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

/// A page's state: the blocks it shows, kept for as long as the app runs.
pub struct State {
    pub page_index: usize,
    blocks: &'static [Block],
}

impl State {
    /// The state of page `page_index`, which shows `blocks`.
    pub fn new(page_index: usize, blocks: &'static [Block]) -> Self {
        Self { page_index, blocks }
    }

    /// Lays the page out under its number.
    pub fn show(&self) {
        println!("page {} of {} blocks", self.page_index, self.blocks.len());
        lay_out(self.blocks, 1);
    }
}

/// What a page is about.
pub struct Meta {
    pub title: &'static str,
    pub description: &'static str,
}

/// A component's page, as native code holds it: a `Box<dyn Page>`.
pub trait Page {
    fn meta(&self) -> &'static Meta;
    fn blocks(&self) -> &'static [Block];
    fn state(&self) -> &State;
    fn page_index(&self) -> usize;
}

/// Shows `page` through each of its methods.
pub fn describe(page: &dyn Page) {
    let meta = page.meta();
    println!("{}: {}", meta.title, meta.description);
    lay_out(page.blocks(), 0);
    page.state().show();
    println!("index {}", page.page_index());
}
