// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../../LICENSE-regex-syntax-MIT.
//! Pinned compiler.rs Utf8Compiler: sorted byte sequences to shared NFA states.
use super::super::{
    fragment::Fragment, Buffer, Builder, Error, Failure, Id, Session, State, Step, Transition,
};
use super::{cache::Cache, ranges::Range};
use rawr_toml::document::Admission;

struct Node {
    transitions: Buffer<Transition>,
    last: Option<Range>,
}
pub(in super::super::super) struct Pending {
    compiled: Cache,
    nodes: Buffer<Node>,
}
impl Pending {
    pub(in super::super::super) fn new() -> Self {
        Self {
            compiled: Cache::new(),
            nodes: Buffer::new(),
        }
    }
    fn clear<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Step<()> {
        self.compiled.clear(session)?;
        session.items(self.nodes.len())?;
        self.nodes.clear();
        Ok(())
    }
}
pub(super) struct Compiler<'a> {
    builder: &'a mut Builder,
    pending: &'a mut Pending,
    target: Id,
}
impl<'a> Compiler<'a> {
    pub(super) fn new<A: Admission>(
        builder: &'a mut Builder,
        pending: &'a mut Pending,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Self, Failure>> {
        let target = match builder.empty(session)? {
            Ok(fragment) => fragment.end,
            Err(failure) => return Ok(Err(failure)),
        };
        pending.clear(session)?;
        pending.nodes.push(
            Node {
                transitions: Buffer::new(),
                last: None,
            },
            session,
        )?;
        Ok(Ok(Self {
            builder,
            pending,
            target,
        }))
    }

    pub(super) fn add<A: Admission>(
        &mut self,
        ranges: &[Range],
        session: &mut Session<'_, A>,
    ) -> Step<Result<(), Failure>> {
        let mut prefix = 0;
        loop {
            session.items(1)?;
            let (Some(range), Some(node)) = (
                ranges.get(prefix),
                self.pending.nodes.as_slice().get(prefix),
            ) else {
                break;
            };
            if node.last != Some(*range) {
                break;
            }
            prefix += 1;
        }
        if prefix >= ranges.len() {
            return session.fail(Error::State);
        }
        if let Err(failure) = self.compile_from(prefix, session)? {
            return Ok(Err(failure));
        }
        self.add_suffix(&ranges[prefix..], session)?;
        Ok(Ok(()))
    }

    pub(super) fn finish<A: Admission>(
        &mut self,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Fragment, Failure>> {
        if let Err(failure) = self.compile_from(0, session)? {
            return Ok(Err(failure));
        }
        session.items(1)?;
        if self.pending.nodes.len() != 1 {
            return session.fail(Error::State);
        }
        let Some(node) = self.pending.nodes.pop() else {
            return session.fail(Error::State);
        };
        if node.last.is_some() {
            return session.fail(Error::State);
        }
        Ok(self
            .compile(node.transitions, session)?
            .map(|start| Fragment {
                start,
                end: self.target,
            }))
    }

    fn compile_from<A: Admission>(
        &mut self,
        from: usize,
        session: &mut Session<'_, A>,
    ) -> Step<Result<(), Failure>> {
        let mut next = self.target;
        loop {
            session.items(1)?;
            if from + 1 >= self.pending.nodes.len() {
                break;
            }
            let Some(mut node) = self.pending.nodes.pop() else {
                return session.fail(Error::State);
            };
            node.freeze(next, session)?;
            next = match self.compile(node.transitions, session)? {
                Ok(id) => id,
                Err(failure) => return Ok(Err(failure)),
            };
        }
        session.items(1)?;
        let Some(node) = self.pending.nodes.as_mut_slice().last_mut() else {
            return session.fail(Error::State);
        };
        node.freeze(next, session)?;
        Ok(Ok(()))
    }

    fn compile<A: Admission>(
        &mut self,
        transitions: Buffer<Transition>,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Id, Failure>> {
        let hash = self
            .pending
            .compiled
            .hash(transitions.as_slice(), session)?;
        if let Some(id) = self
            .pending
            .compiled
            .get(transitions.as_slice(), hash, session)?
        {
            return Ok(Ok(id));
        }
        // The source clones edges for the builder while retaining the cache key.
        let mut copy = Buffer::new();
        copy.reserve_more(transitions.len(), session)?;
        for transition in transitions.as_slice() {
            session.items(1)?;
            copy.push(*transition, session)?;
        }
        let id = match self
            .builder
            .add(State::Sparse { transitions: copy }, session)?
        {
            Ok(id) => id,
            Err(failure) => return Ok(Err(failure)),
        };
        self.pending.compiled.set(transitions, hash, id, session)?;
        Ok(Ok(id))
    }

    fn add_suffix<A: Admission>(
        &mut self,
        ranges: &[Range],
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        let Some(first) = ranges.first() else {
            return session.fail(Error::State);
        };
        let Some(node) = self.pending.nodes.as_mut_slice().last_mut() else {
            return session.fail(Error::State);
        };
        if node.last.is_some() {
            return session.fail(Error::State);
        }
        node.last = Some(*first);
        for range in &ranges[1..] {
            session.items(1)?;
            self.pending.nodes.push(
                Node {
                    transitions: Buffer::new(),
                    last: Some(*range),
                },
                session,
            )?;
        }
        Ok(())
    }
}
impl Node {
    fn freeze<A: Admission>(&mut self, next: Id, session: &mut Session<'_, A>) -> Step<()> {
        session.items(1)?;
        if let Some(last) = self.last.take() {
            self.transitions.push(
                Transition {
                    start: last.start,
                    end: last.end,
                    next,
                },
                session,
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::ranges::Ranges;
    use super::*;
    use crate::input::membership::manifest::{reference::Account, Stop};
    fn build<A: Admission>(
        builder: &mut Builder,
        pending: &mut Pending,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Fragment, Failure>> {
        let mut compiler = match Compiler::new(builder, pending, session)? {
            Ok(compiler) => compiler,
            Err(failure) => return Ok(Err(failure)),
        };
        let mut ranges = Ranges::new('\0', char::MAX, session)?;
        while let Some(sequence) = ranges.next(session)? {
            if let Err(failure) = compiler.add(sequence.as_slice(), session)? {
                return Ok(Err(failure));
            }
        }
        compiler.finish(session)
    }
    fn accepts(builder: &Builder, fragment: &Fragment, bytes: &[u8]) -> bool {
        let mut id = fragment.start;
        for byte in bytes {
            let State::Sparse { transitions } = &builder.states.as_slice()[id.0 as usize] else {
                return false;
            };
            let Some(transition) = transitions
                .as_slice()
                .iter()
                .find(|t| t.start <= *byte && *byte <= t.end)
            else {
                return false;
            };
            id = transition.next;
        }
        id == fragment.end
    }
    #[test]
    fn compiled_sequences_cover_unicode_and_reuse_suffix_states() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut builder = Builder::new();
        let mut pending = Pending::new();
        let fragment = build(&mut builder, &mut pending, &mut session)
            .unwrap()
            .unwrap();
        for scalar in 0..=u32::from(char::MAX) {
            let Some(c) = char::from_u32(scalar) else {
                continue;
            };
            let mut bytes = [0; 4];
            assert!(accepts(
                &builder,
                &fragment,
                c.encode_utf8(&mut bytes).as_bytes()
            ));
        }
        for bytes in [
            &[][..],
            &[0xED, 0xA0, 0x80],
            &[0xF4, 0x90, 0x80, 0x80],
            b"ab",
        ] {
            assert!(!accepts(&builder, &fragment, bytes));
        }
        // Multiple parent edges lead to the same compiled continuation.
        let mut targets = Vec::new();
        let mut shared = false;
        for state in builder.states.as_slice() {
            if let State::Sparse { transitions } = state {
                for edge in transitions.as_slice() {
                    shared |= edge.next != fragment.end && targets.contains(&edge.next);
                    targets.push(edge.next);
                }
            }
        }
        assert!(shared);
        let next = build(&mut builder, &mut pending, &mut session)
            .unwrap()
            .unwrap();
        assert_ne!(fragment.end, next.end);
        assert!(accepts(&builder, &next, "é".as_bytes()));
    }
    #[test]
    fn size_failure_does_not_publish_cache_entry() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut builder = Builder::new();
        builder
            .set_size_limit(Some(super::super::super::SOURCE_STATE_BYTES), &mut session)
            .unwrap()
            .unwrap();
        let mut pending = Pending::new();
        assert!(matches!(
            build(&mut builder, &mut pending, &mut session).unwrap(),
            Err(Failure::SizeLimit(_))
        ));
        assert!(builder.states.len() > 1);
        let State::Sparse { transitions } = &builder.states.as_slice()[1] else {
            panic!("inserted sparse state");
        };
        let hash = pending
            .compiled
            .hash(transitions.as_slice(), &mut session)
            .unwrap();
        assert_eq!(
            pending
                .compiled
                .get(transitions.as_slice(), hash, &mut session)
                .unwrap(),
            None
        );
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
    #[test]
    fn refusal_is_terminal_through_compiler_and_cache() {
        // Retain initialized cache and continuation capacity so every sweep
        // starts from the same storage shape, without quadratically repeating
        // the cache's 10,000-entry initialization.
        let mut account = Account;
        let mut pending = Pending::new();
        build(
            &mut Builder::new(),
            &mut pending,
            &mut Session::new(&mut account),
        )
        .unwrap()
        .unwrap();
        let mut count = Count {
            calls: 0,
            refuse: usize::MAX,
        };
        build(
            &mut Builder::new(),
            &mut pending,
            &mut Session::new(&mut count),
        )
        .unwrap()
        .unwrap();
        for refuse in 0..count.calls {
            let mut count = Count { calls: 0, refuse };
            let mut session = Session::new(&mut count);
            assert!(matches!(
                build(&mut Builder::new(), &mut pending, &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(session.check(), Err(Stop::Refused)));
            assert_eq!(count.calls, refuse + 1);
        }
    }
}
