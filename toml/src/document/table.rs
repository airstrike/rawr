//! An ordered AVL map. Search links and insertion-order links are independent.
//! All links are indices in one prospectively allocated vector; no per-node
//! allocation, hashing, random seed, or infallible collection growth is used.

use core::cmp::Ordering;

use super::storage::{self, Buffer, Shared};
use super::{Admission, Key, Request, Spanned, Value};

type Link = Option<usize>;

#[derive(Debug)]
struct Node<'i> {
    entry: Option<(Key<'i>, Spanned<Value<'i>>)>,
    left: Link,
    right: Link,
    height: u8,
    previous: Link,
    next: Link,
}

/// Insertion-ordered semantic table with decoded-key equality.
#[derive(Debug, Default)]
pub struct Table<'i> {
    nodes: Buffer<Node<'i>>,
    root: Link,
    first: Link,
    last: Link,
    free: Link,
    len: usize,
    disposal: usize,
    pub(super) implicit: bool,
    pub(super) dotted: bool,
    pub(super) inline: bool,
}

impl<'i> Table<'i> {
    pub(super) fn marked(implicit: bool, dotted: bool, inline: bool) -> Self {
        let mut table = Self::default();
        table.implicit = implicit;
        table.dotted = dotted;
        table.inline = inline;
        table
    }

    pub(super) fn slot_count(&self) -> usize {
        self.nodes.len()
    }

    pub(super) fn disposal(&mut self) -> &mut usize {
        &mut self.disposal
    }

    pub(super) fn slot_mut(&mut self, index: usize) -> Option<&mut Value<'i>> {
        self.nodes
            .as_mut_slice()
            .get_mut(index)?
            .entry
            .as_mut()
            .map(|entry| &mut entry.1.value)
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn is_implicit(&self) -> bool {
        self.implicit
    }

    pub fn is_dotted(&self) -> bool {
        self.dotted
    }

    pub fn is_inline(&self) -> bool {
        self.inline
    }

    pub fn get(&self, key: &str) -> Option<&Spanned<Value<'i>>> {
        self.find(key).map(|index| &self.entry(index).1)
    }

    pub fn iter(&self) -> Iter<'_, 'i> {
        Iter {
            table: self,
            next: self.first,
            remaining: self.len,
        }
    }

    pub(super) fn find(&self, key: &str) -> Link {
        let mut next = self.root;
        while let Some(index) = next {
            match key.cmp(self.entry(index).0.as_str()) {
                Ordering::Less => next = self.nodes.as_slice()[index].left,
                Ordering::Greater => next = self.nodes.as_slice()[index].right,
                Ordering::Equal => return Some(index),
            }
        }
        None
    }

    pub(super) fn entry(&self, index: usize) -> &(Key<'i>, Spanned<Value<'i>>) {
        self.nodes.as_slice()[index]
            .entry
            .as_ref()
            .expect("live AVL index")
    }

    pub(super) fn entry_mut(&mut self, index: usize) -> &mut (Key<'i>, Spanned<Value<'i>>) {
        self.nodes.as_mut_slice()[index]
            .entry
            .as_mut()
            .expect("live AVL index")
    }

    /// Replacement retains the original key object and its order position.
    /// Vacant insertion appends, including when it reuses a removed slot.
    pub(super) fn insert<A: Admission>(
        &mut self,
        key: Key<'i>,
        value: Spanned<Value<'i>>,
        session: &Shared<'_, A>,
    ) -> Option<usize> {
        if let Some(index) = self.find(key.as_str()) {
            if !storage::admit(session, Request::Items(1)) {
                return None;
            }
            self.entry_mut(index).1 = value;
            return Some(index);
        }
        let node = Node {
            entry: Some((key, value)),
            left: None,
            right: None,
            height: 1,
            previous: self.last,
            next: None,
        };
        let index = match self.free {
            Some(index) => {
                if !storage::admit(session, Request::Items(1)) {
                    return None;
                }
                self.free = self.nodes.as_slice()[index].next;
                self.nodes.as_mut_slice()[index] = node;
                index
            }
            None => {
                let index = self.nodes.len();
                if !self.nodes.push(node, session) {
                    return None;
                }
                index
            }
        };
        if let Some(last) = self.last {
            self.nodes.as_mut_slice()[last].next = Some(index);
        } else {
            self.first = Some(index);
        }
        self.last = Some(index);
        self.len += 1;
        self.root = Some(self.insert_at(self.root, index));
        Some(index)
    }

    /// The pinned shift-remove operation: detach from order and search indexes.
    pub(super) fn remove(&mut self, key: &str) -> Option<(Key<'i>, Spanned<Value<'i>>)> {
        let index = self.find(key)?;
        self.root = self.remove_at(self.root, key);
        let previous = self.nodes.as_slice()[index].previous;
        let next = self.nodes.as_slice()[index].next;
        if let Some(previous) = previous {
            self.nodes.as_mut_slice()[previous].next = next;
        } else {
            self.first = next;
        }
        if let Some(next) = next {
            self.nodes.as_mut_slice()[next].previous = previous;
        } else {
            self.last = previous;
        }
        self.len -= 1;
        let node = &mut self.nodes.as_mut_slice()[index];
        node.left = None;
        node.right = None;
        node.previous = None;
        node.next = self.free;
        self.free = Some(index);
        node.entry.take()
    }

    fn height(&self, index: Link) -> u8 {
        index.map_or(0, |index| self.nodes.as_slice()[index].height)
    }

    fn update(&mut self, index: usize) {
        let node = &self.nodes.as_slice()[index];
        let height = 1 + self.height(node.left).max(self.height(node.right));
        self.nodes.as_mut_slice()[index].height = height;
    }

    fn balance(&self, index: usize) -> i16 {
        let node = &self.nodes.as_slice()[index];
        i16::from(self.height(node.left)) - i16::from(self.height(node.right))
    }

    fn rotate_left(&mut self, index: usize) -> usize {
        let right = self.nodes.as_slice()[index]
            .right
            .expect("right-heavy AVL node");
        self.nodes.as_mut_slice()[index].right = self.nodes.as_slice()[right].left;
        self.nodes.as_mut_slice()[right].left = Some(index);
        self.update(index);
        self.update(right);
        right
    }

    fn rotate_right(&mut self, index: usize) -> usize {
        let left = self.nodes.as_slice()[index]
            .left
            .expect("left-heavy AVL node");
        self.nodes.as_mut_slice()[index].left = self.nodes.as_slice()[left].right;
        self.nodes.as_mut_slice()[left].right = Some(index);
        self.update(index);
        self.update(left);
        left
    }

    fn rebalance(&mut self, index: usize) -> usize {
        self.update(index);
        match self.balance(index) {
            balance if balance > 1 => {
                let left = self.nodes.as_slice()[index]
                    .left
                    .expect("left-heavy AVL node");
                if self.balance(left) < 0 {
                    let left = self.rotate_left(left);
                    self.nodes.as_mut_slice()[index].left = Some(left);
                }
                self.rotate_right(index)
            }
            balance if balance < -1 => {
                let right = self.nodes.as_slice()[index]
                    .right
                    .expect("right-heavy AVL node");
                if self.balance(right) > 0 {
                    let right = self.rotate_right(right);
                    self.nodes.as_mut_slice()[index].right = Some(right);
                }
                self.rotate_left(index)
            }
            _ => index,
        }
    }

    fn insert_at(&mut self, root: Link, inserted: usize) -> usize {
        let Some(root) = root else { return inserted };
        if self.entry(inserted).0.as_str() < self.entry(root).0.as_str() {
            let left = self.insert_at(self.nodes.as_slice()[root].left, inserted);
            self.nodes.as_mut_slice()[root].left = Some(left);
        } else {
            let right = self.insert_at(self.nodes.as_slice()[root].right, inserted);
            self.nodes.as_mut_slice()[root].right = Some(right);
        }
        self.rebalance(root)
    }

    fn remove_at(&mut self, root: Link, key: &str) -> Link {
        let root = root?;
        match key.cmp(self.entry(root).0.as_str()) {
            Ordering::Less => {
                let left = self.remove_at(self.nodes.as_slice()[root].left, key);
                self.nodes.as_mut_slice()[root].left = left;
            }
            Ordering::Greater => {
                let right = self.remove_at(self.nodes.as_slice()[root].right, key);
                self.nodes.as_mut_slice()[root].right = right;
            }
            Ordering::Equal => {
                let node = &self.nodes.as_slice()[root];
                let (Some(left), Some(right)) = (node.left, node.right) else {
                    return node.left.or(node.right);
                };
                let (right, successor) = self.detach_min(right);
                self.nodes.as_mut_slice()[successor].left = Some(left);
                self.nodes.as_mut_slice()[successor].right = right;
                return Some(self.rebalance(successor));
            }
        }
        Some(self.rebalance(root))
    }

    fn detach_min(&mut self, root: usize) -> (Link, usize) {
        let Some(left) = self.nodes.as_slice()[root].left else {
            return (self.nodes.as_slice()[root].right, root);
        };
        let (left, minimum) = self.detach_min(left);
        self.nodes.as_mut_slice()[root].left = left;
        (Some(self.rebalance(root)), minimum)
    }
}

impl Drop for Table<'_> {
    fn drop(&mut self) {
        super::dispose::table(self);
    }
}

pub struct Iter<'a, 'i> {
    table: &'a Table<'i>,
    next: Link,
    remaining: usize,
}

impl<'a, 'i> Iterator for Iter<'a, 'i> {
    type Item = (&'a Key<'i>, &'a Spanned<Value<'i>>);

    fn next(&mut self) -> Option<Self::Item> {
        let index = self.next?;
        self.next = self.table.nodes.as_slice()[index].next;
        self.remaining -= 1;
        let (key, value) = self.table.entry(index);
        Some((key, value))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for Iter<'_, '_> {}

#[cfg(test)]
impl Table<'_> {
    pub(super) fn check_avl(&self) {
        fn check(table: &Table<'_>, root: Link, low: Option<&str>, high: Option<&str>) -> u8 {
            let Some(root) = root else { return 0 };
            let node = &table.nodes.as_slice()[root];
            let key = table.entry(root).0.as_str();
            assert!(low.is_none_or(|low| low < key));
            assert!(high.is_none_or(|high| key < high));
            let left = check(table, node.left, low, Some(key));
            let right = check(table, node.right, Some(key), high);
            assert!(left.abs_diff(right) <= 1);
            assert_eq!(node.height, 1 + left.max(right));
            node.height
        }
        check(self, self.root, None, None);
        assert_eq!(self.iter().count(), self.len);
    }
}
