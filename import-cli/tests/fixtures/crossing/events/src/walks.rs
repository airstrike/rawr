//! A walk of a holder's nodes, as iced's `widget::Tree` and `Layout`: the
//! crate compiles the nodes and the spots, and a holder's impl walks them
//! through `Spot::iter_mut` and `Spot::iter`, which borrow the children a
//! node lends.

use crate::tinted::Diffed;
use crate::visits::Collect;

/// A node of a holder's tree, as iced's `widget::Tree`: its children are
/// public.
#[derive(Debug, Default)]
pub struct Node {
    pub children: Vec<Node>,
    marks: Vec<String>,
}

impl Node {
    /// A node over `children`.
    #[must_use]
    pub fn new(children: Vec<Node>) -> Self {
        Self {
            children,
            marks: Vec::new(),
        }
    }

    /// Notes `mark` on the node.
    pub fn mark(&mut self, mark: &str) {
        self.marks.push(mark.to_owned());
    }

    /// How many marks the node has.
    #[must_use]
    pub fn marked(&self) -> usize {
        self.marks.len()
    }

    /// Each mark of the node and its descendants, depth first, after its
    /// depth.
    #[must_use]
    pub fn listing(&self) -> Vec<String> {
        let mut listing = Vec::new();
        self.list(0, &mut listing);
        listing
    }

    fn list(&self, depth: usize, listing: &mut Vec<String>) {
        for mark in &self.marks {
            listing.push(format!("{depth}:{mark}"));
        }
        for child in &self.children {
            child.list(depth + 1, listing);
        }
    }
}

/// A node lifted out of a holder for as long as the holder and the node
/// are borrowed, as iced's `overlay::Element<'a, ..>` borrows its widget
/// and tree.
pub struct Lifted<'b> {
    pub node: &'b mut Node,
}

/// Where a node is, as iced's `Layout`: a copy the crate makes for each
/// child it walks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spot {
    depth: usize,
    width: usize,
}

impl Spot {
    /// The root's spot.
    #[must_use]
    pub fn root() -> Self {
        Self { depth: 0, width: 1 }
    }

    /// How deep the spot is, from 0.
    #[must_use]
    pub fn depth(self) -> usize {
        self.depth
    }

    /// How many children the spot's node has.
    #[must_use]
    pub fn width(self) -> usize {
        self.width
    }

    /// Each child's spot and the child, lent mutably, as iced's
    /// `Layout::iter_mut`.
    pub fn iter_mut(
        self,
        children: &mut [Node],
    ) -> impl DoubleEndedIterator<Item = (Self, &mut Node)> + ExactSizeIterator {
        children
            .iter_mut()
            .map(move |child| (self.child(child), child))
    }

    /// Each child's spot and the child, as iced's `Layout::iter`.
    pub fn iter(
        self,
        children: &[Node],
    ) -> impl DoubleEndedIterator<Item = (Self, &Node)> + ExactSizeIterator {
        children.iter().map(move |child| (self.child(child), child))
    }

    fn child(self, child: &Node) -> Self {
        Self {
            depth: self.depth + 1,
            width: child.children.len(),
        }
    }
}

/// A chain of `depth` nodes below a root, each with one child.
fn chain(depth: usize) -> Node {
    (0..depth).fold(Node::default(), |child, _| Node::new(vec![child]))
}

/// What `item`'s walk over a chain of nodes the crate owns marks, what
/// its visit notes, and what it sees of the marked chain after, as iced's
/// `UserInterface` lends the root widget it owns its `Tree` and a
/// `Layout`. The crate compiles it.
pub fn walked<M>(mut item: impl Diffed<M>) -> Vec<String> {
    let mut root = chain(3);
    let mut collect = Collect::default();
    item.walk(Spot::root(), &mut root, &mut collect);
    let mut walked = root.listing();
    walked.extend(collect.labels());
    walked.push(item.seen(Spot::root(), &root));
    walked
}
