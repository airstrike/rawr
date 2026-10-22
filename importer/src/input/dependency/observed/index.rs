//! Flat AVL duplicate lookup over immutable append-order payloads.

use std::cmp::Ordering;

use crate::input::stored::{observe::slots::Slots, Budget, Error, Result};

pub(in crate::input) struct Index {
    nodes: Slots<Node>,
    root: Option<usize>,
}

struct Node {
    left: Option<usize>,
    right: Option<usize>,
    parent: Option<usize>,
    height: u8,
}

#[derive(Clone, Copy)]
enum Side {
    Left,
    Right,
}

/// A vacant edge in this exact append-only index extent.
pub(in crate::input) struct Vacant {
    parent: Option<usize>,
    side: Side,
    count: usize,
}

impl Side {
    fn opposite(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

impl Node {
    fn child(&self, side: Side) -> Option<usize> {
        match side {
            Side::Left => self.left,
            Side::Right => self.right,
        }
    }

    fn set(&mut self, side: Side, child: Option<usize>) {
        match side {
            Side::Left => self.left = child,
            Side::Right => self.right = child,
        }
    }
}

impl Index {
    pub(in crate::input) const fn new() -> Self {
        Self {
            nodes: Slots::new(),
            root: None,
        }
    }

    /// Payload ordinals remain fixed until the index is discarded.
    pub(in crate::input) fn locate<T>(
        &self,
        values: &[T],
        compare: impl Fn(&T) -> Ordering,
    ) -> std::result::Result<Vacant, usize> {
        let mut current = self.root;
        let mut parent = None;
        let mut side = Side::Left;
        while let Some(index) = current {
            side = match compare(&values[index]) {
                Ordering::Less => Side::Right,
                Ordering::Greater => Side::Left,
                Ordering::Equal => return Err(index),
            };
            parent = current;
            current = self.nodes.as_slice()[index].child(side);
        }
        Ok(Vacant {
            parent,
            side,
            count: self.nodes.as_slice().len(),
        })
    }

    /// Append one index node after its payload, then rebalance without scratch.
    pub(in crate::input) fn insert(
        &mut self,
        vacant: Vacant,
        budget: &mut Budget<'_>,
    ) -> Result<()> {
        budget.check()?;
        let index = self.nodes.as_slice().len();
        if index != vacant.count {
            return Err(Error::Encoding(
                "duplicate index extent changed before insertion",
            ));
        }
        self.nodes.push(
            Node {
                left: None,
                right: None,
                parent: vacant.parent,
                height: 1,
            },
            budget,
        )?;
        if let Some(parent) = vacant.parent {
            self.nodes.as_mut_slice()[parent].set(vacant.side, Some(index));
        } else {
            self.root = Some(index);
        }
        self.rebalance(vacant.parent)
            .map_err(|error| budget.failure(error))
    }

    fn height(&self, index: Option<usize>) -> u8 {
        index.map_or(0, |index| self.nodes.as_slice()[index].height)
    }

    fn update(&mut self, index: usize) -> Result<()> {
        let node = &self.nodes.as_slice()[index];
        let height = self
            .height(node.left)
            .max(self.height(node.right))
            .checked_add(1)
            .ok_or(Error::Overflow)?;
        self.nodes.as_mut_slice()[index].height = height;
        Ok(())
    }

    fn rebalance(&mut self, mut current: Option<usize>) -> Result<()> {
        while let Some(index) = current {
            self.update(index)?;
            let node = &self.nodes.as_slice()[index];
            let balance = i16::from(self.height(node.left)) - i16::from(self.height(node.right));
            let heavy = if balance > 1 {
                Some(Side::Left)
            } else if balance < -1 {
                Some(Side::Right)
            } else {
                None
            };
            let root = if let Some(side) = heavy {
                let child = node
                    .child(side)
                    .ok_or(Error::Encoding("unbalanced index lacks its heavy child"))?;
                let node = &self.nodes.as_slice()[child];
                if self.height(node.child(side)) < self.height(node.child(side.opposite())) {
                    self.rotate(child, side.opposite())?;
                }
                self.rotate(index, side)?
            } else {
                index
            };
            current = self.nodes.as_slice()[root].parent;
        }
        Ok(())
    }

    fn rotate(&mut self, root: usize, side: Side) -> Result<usize> {
        let node = &self.nodes.as_slice()[root];
        let parent = node.parent;
        let promoted = node
            .child(side)
            .ok_or(Error::Encoding("index rotation lacks its child"))?;
        let middle = self.nodes.as_slice()[promoted].child(side.opposite());
        if let Some(parent) = parent {
            let parent = &mut self.nodes.as_mut_slice()[parent];
            if parent.left == Some(root) {
                parent.left = Some(promoted);
            } else if parent.right == Some(root) {
                parent.right = Some(promoted);
            } else {
                return Err(Error::Encoding(
                    "index parent does not own the rotated node",
                ));
            }
        } else {
            self.root = Some(promoted);
        }
        self.nodes.as_mut_slice()[promoted].parent = parent;
        self.nodes.as_mut_slice()[root].set(side, middle);
        if let Some(middle) = middle {
            self.nodes.as_mut_slice()[middle].parent = Some(root);
        }
        self.nodes.as_mut_slice()[promoted].set(side.opposite(), Some(root));
        self.nodes.as_mut_slice()[root].parent = Some(promoted);
        self.update(root)?;
        self.update(promoted)?;
        Ok(promoted)
    }
}

#[cfg(test)]
pub(super) const fn node_bytes() -> usize {
    std::mem::size_of::<Node>()
}

#[cfg(test)]
mod tests;
