// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../LICENSE-regex-syntax-MIT.
//! Pinned literal_trie.rs insertion and nonrecursive NFA compilation.
use super::{
    fragment::Fragment, Buffer, Builder, Error, Failure, Id, Session, State, Step, Transition,
    INDEX_LIMIT,
};
use rawr_toml::document::Admission;

#[derive(Clone, Copy)]
struct Edge {
    byte: u8,
    next: usize,
}
struct Node {
    edges: Buffer<Edge>,
    chunks: Buffer<(usize, usize)>,
}
impl Node {
    fn empty() -> Self {
        Self {
            edges: Buffer::new(),
            chunks: Buffer::new(),
        }
    }
    fn active(&self) -> usize {
        self.chunks.as_slice().last().map_or(0, |pair| pair.1)
    }
    fn mark<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Step<()> {
        session.items(1)?;
        if self.edges.len() == 0 && self.chunks.len() != 0 {
            return Ok(());
        }
        self.chunks
            .push((self.active(), self.edges.len()), session)?;
        Ok(())
    }
}
pub(in super::super) struct Trie {
    nodes: Buffer<Node>,
    reverse: bool,
}
impl Trie {
    pub(in super::super) fn new<A: Admission>(
        reverse: bool,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        let mut nodes = Buffer::new();
        nodes.push(Node::empty(), session)?;
        Ok(Self { nodes, reverse })
    }
    pub(in super::super) fn add<A: Admission>(
        &mut self,
        bytes: &[u8],
        session: &mut Session<'_, A>,
    ) -> Step<Result<(), Failure>> {
        let mut previous = 0;
        let mut bytes = bytes.iter().copied();
        loop {
            session.items(1)?;
            let Some(byte) = (if self.reverse {
                bytes.next_back()
            } else {
                bytes.next()
            }) else {
                break;
            };
            session.items(1)?;
            let Some(node) = self.nodes.as_slice().get(previous) else {
                return session.fail(Error::State);
            };
            let start = node.active();
            let active = &node.edges.as_slice()[start..];
            // No ownership or effects in the standard slice search. Admit a
            // conservative comparison envelope before entering it.
            session.items(active.len())?;
            match active.binary_search_by_key(&byte, |edge| edge.byte) {
                Ok(index) => previous = active[index].next,
                Err(index) => {
                    if self.nodes.len() >= INDEX_LIMIT {
                        return Ok(Err(Failure::TooManyStates(self.nodes.len())));
                    }
                    let next = self.nodes.len();
                    self.nodes.push(Node::empty(), session)?;
                    let node = &mut self.nodes.as_mut_slice()[previous];
                    let index = node.active() + index;
                    session.items(node.edges.len() - index + 1)?;
                    node.edges.push(Edge { byte, next }, session)?;
                    node.edges.as_mut_slice()[index..].rotate_right(1);
                    previous = next;
                }
            }
        }
        self.nodes.as_mut_slice()[previous].mark(session)?;
        Ok(Ok(()))
    }
    pub(in super::super) fn compile<A: Admission>(
        &self,
        builder: &mut Builder,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Fragment, Failure>> {
        let end = match builder.empty(session)? {
            Ok(f) => f.end,
            Err(e) => return Ok(Err(e)),
        };
        let mut stack = Buffer::new();
        let mut frame = Frame::new(&self.nodes.as_slice()[0], session)?;
        loop {
            session.items(1)?;
            if let Some(edge) = frame.transitions.next() {
                session.items(1)?;
                let Some(node) = self.nodes.as_slice().get(edge.next) else {
                    return session.fail(Error::State);
                };
                if node.edges.len() == 0 {
                    frame.sparse.push(
                        Transition {
                            start: edge.byte,
                            end: edge.byte,
                            next: end,
                        },
                        session,
                    )?;
                } else {
                    frame.sparse.push(
                        Transition {
                            start: edge.byte,
                            end: edge.byte,
                            next: Id::ZERO,
                        },
                        session,
                    )?;
                    stack.push(frame, session)?;
                    frame = Frame::new(node, session)?;
                }
                continue;
            }
            if frame.sparse.len() != 0 {
                let state = if frame.sparse.len() == 1 {
                    let Some(trans) = frame.sparse.pop() else {
                        return session.fail(Error::State);
                    };
                    State::ByteRange { trans }
                } else {
                    State::Sparse {
                        transitions: std::mem::replace(&mut frame.sparse, Buffer::new()),
                    }
                };
                let chunk = match builder.add(state, session)? {
                    Ok(id) => id,
                    Err(e) => return Ok(Err(e)),
                };
                frame.union.push(chunk, session)?;
            }
            if let Some(chunk) = frame.chunks.next(session)? {
                frame.union.push(end, session)?;
                frame.transitions = chunk.iter();
                continue;
            }
            let start = match builder.add(
                State::Union {
                    alternates: frame.union,
                },
                session,
            )? {
                Ok(id) => id,
                Err(e) => return Ok(Err(e)),
            };
            session.items(1)?;
            match stack.pop() {
                None => return Ok(Ok(Fragment { start, end })),
                Some(mut parent) => {
                    let Some(edge) = parent.sparse.as_mut_slice().last_mut() else {
                        return session.fail(Error::State);
                    };
                    edge.next = start;
                    frame = parent;
                }
            }
        }
    }
}
struct Chunks<'a> {
    edges: &'a [Edge],
    chunks: std::slice::Iter<'a, (usize, usize)>,
    active: Option<&'a [Edge]>,
}
impl<'a> Chunks<'a> {
    fn next<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Step<Option<&'a [Edge]>> {
        session.items(1)?;
        if let Some(&(start, end)) = self.chunks.next() {
            return Ok(Some(&self.edges[start..end]));
        }
        Ok(self.active.take())
    }
}
struct Frame<'a> {
    chunks: Chunks<'a>,
    transitions: std::slice::Iter<'a, Edge>,
    union: Buffer<Id>,
    sparse: Buffer<Transition>,
}
impl<'a> Frame<'a> {
    fn new<A: Admission>(node: &'a Node, session: &mut Session<'_, A>) -> Step<Self> {
        session.items(1)?;
        let mut chunks = Chunks {
            edges: node.edges.as_slice(),
            chunks: node.chunks.as_slice().iter(),
            active: Some(&node.edges.as_slice()[node.active()..]),
        };
        let Some(chunk) = chunks.next(session)? else {
            return session.fail(Error::State);
        };
        Ok(Self {
            chunks,
            transitions: chunk.iter(),
            union: Buffer::new(),
            sparse: Buffer::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference::Account, Stop};
    fn accepts(builder: &Builder, fragment: &Fragment, bytes: &[u8]) -> bool {
        let mut pending = vec![(fragment.start, 0)];
        while let Some((id, pos)) = pending.pop() {
            if id == fragment.end {
                if pos == bytes.len() {
                    return true;
                } else {
                    continue;
                }
            }
            match &builder.states.as_slice()[id.0 as usize] {
                State::Union { alternates } => {
                    pending.extend(alternates.as_slice().iter().map(|id| (*id, pos)))
                }
                State::ByteRange { trans }
                    if bytes
                        .get(pos)
                        .is_some_and(|b| trans.start <= *b && *b <= trans.end) =>
                {
                    pending.push((trans.next, pos + 1))
                }
                State::Sparse { transitions } => {
                    for t in transitions.as_slice() {
                        if bytes.get(pos).is_some_and(|b| t.start <= *b && *b <= t.end) {
                            pending.push((t.next, pos + 1));
                        }
                    }
                }
                _ => {}
            }
        }
        false
    }
    #[test]
    fn trie_compiles_prefixes_duplicates_and_both_directions() {
        for reverse in [false, true] {
            let mut account = Account;
            let mut session = Session::new(&mut account);
            let mut trie = Trie::new(reverse, &mut session).unwrap();
            for literal in [b"foo".as_slice(), b"fo", b"bar", b"foo", b"fox"] {
                trie.add(literal, &mut session).unwrap().unwrap();
            }
            let mut builder = Builder::new();
            let fragment = trie.compile(&mut builder, &mut session).unwrap().unwrap();
            for (literal, expected) in [
                (b"foo".as_slice(), true),
                (b"fo", true),
                (b"bar", true),
                (b"fox", true),
                (b"f", false),
                (b"food", false),
                (b"", false),
            ] {
                let mut bytes = literal.to_vec();
                if reverse {
                    bytes.reverse();
                }
                assert_eq!(accepts(&builder, &fragment, &bytes), expected);
            }
        }
    }
    #[test]
    fn marking_a_prefix_freezes_earlier_edges_and_leaf_duplicate_is_free() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut trie = Trie::new(false, &mut session).unwrap();
        for literal in [b"ab".as_slice(), b"a", b"ac"] {
            trie.add(literal, &mut session).unwrap().unwrap();
        }
        let a = trie.nodes.as_slice()[0].edges.as_slice()[0].next;
        let node = &trie.nodes.as_slice()[a];
        assert_eq!(node.chunks.as_slice(), &[(0, 1)]);
        assert_eq!(node.edges.as_slice()[0].byte, b'b');
        assert_eq!(node.edges.as_slice()[1].byte, b'c');
        let count = trie.nodes.len();
        trie.add(b"ac", &mut session).unwrap().unwrap();
        assert_eq!(trie.nodes.len(), count);
        let c = trie.nodes.as_slice()[a].edges.as_slice()[1].next;
        assert_eq!(trie.nodes.as_slice()[c].chunks.len(), 1);
    }
    struct Count {
        calls: usize,
        refuse: usize,
    }
    impl Admission for Count {
        type Error = ();
        fn admit(&mut self, _: rawr_toml::document::Request) -> Result<(), ()> {
            let call = self.calls;
            self.calls += 1;
            if call == self.refuse {
                Err(())
            } else {
                Ok(())
            }
        }
    }
    fn exercise<A: Admission>(session: &mut Session<'_, A>) -> Step<()> {
        let mut trie = Trie::new(false, session)?;
        for literal in [b"ab".as_slice(), b"a", b"ac", b"z"] {
            assert!(trie.add(literal, session)?.is_ok());
        }
        assert!(trie.compile(&mut Builder::new(), session)?.is_ok());
        Ok(())
    }
    #[test]
    fn every_refusal_stops_trie_insertion_or_compilation() {
        let mut count = Count {
            calls: 0,
            refuse: usize::MAX,
        };
        exercise(&mut Session::new(&mut count)).unwrap();
        for refuse in 0..count.calls {
            let mut count = Count { calls: 0, refuse };
            let mut session = Session::new(&mut count);
            assert!(matches!(exercise(&mut session), Err(Stop::Refused)));
            assert!(matches!(session.check(), Err(Stop::Refused)));
            assert_eq!(count.calls, refuse + 1);
        }
    }
}
