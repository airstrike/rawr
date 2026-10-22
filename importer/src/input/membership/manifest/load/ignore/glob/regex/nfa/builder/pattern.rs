// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../LICENSE-regex-syntax-MIT.
//! Pinned pattern registration and the emitter's implicit unnamed group zero.
use super::{Buffer, Builder, Error, Failure, Id, Session, State, Step, INDEX_LIMIT};
use rawr_toml::document::Admission;

impl Builder {
    pub(in super::super) fn clear<A: Admission>(
        &mut self,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.check()?;
        self.pattern = None;
        session.items(self.states.len())?;
        self.states.clear();
        self.starts.clear();
        // The source retains the outer vector, dropping each inner capture
        // vector. No named captures can be emitted by the glob producer.
        session.items(self.captures.len())?;
        for captures in self.captures.as_slice() {
            session.items(captures.len())?;
        }
        self.captures.clear();
        self.memory_states = 0;
        Ok(())
    }
    pub(in super::super) fn start_pattern<A: Admission>(
        &mut self,
        session: &mut Session<'_, A>,
    ) -> Step<Result<u32, Failure>> {
        session.items(1)?;
        if self.pattern.is_some() {
            return session.fail(Error::State);
        }
        let proposed = self.starts.len();
        if proposed >= INDEX_LIMIT {
            return Ok(Err(Failure::TooManyPatterns(proposed)));
        }
        let pattern = proposed as u32;
        self.pattern = Some(pattern);
        self.starts.push(Id::ZERO, session)?;
        Ok(Ok(pattern))
    }
    pub(in super::super) fn finish_pattern<A: Admission>(
        &mut self,
        start: Id,
        session: &mut Session<'_, A>,
    ) -> Step<u32> {
        let pattern = self.current_pattern(session)?;
        let Some(slot) = self.starts.as_mut_slice().get_mut(pattern as usize) else {
            return session.fail(Error::State);
        };
        *slot = start;
        self.pattern = None;
        Ok(pattern)
    }
    fn current_pattern<A: Admission>(&self, session: &mut Session<'_, A>) -> Step<u32> {
        session.items(1)?;
        match self.pattern {
            Some(pattern) => Ok(pattern),
            None => session.fail(Error::State),
        }
    }
    pub(in super::super) fn capture_start<A: Admission>(
        &mut self,
        next: Id,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Id, Failure>> {
        let pattern = self.current_pattern(session)?;
        // Compiler::c_cap supplies group_index=0 and name=None around each
        // pattern. The emitted grammar has no explicit capture groups.
        while pattern as usize >= self.captures.len() {
            self.captures.push(Buffer::new(), session)?;
        }
        let Some(captures) = self.captures.as_mut_slice().get_mut(pattern as usize) else {
            return session.fail(Error::State);
        };
        if captures.len() == 0 {
            captures.push(None, session)?;
        }
        self.add(
            State::CaptureStart {
                pattern,
                group: 0,
                next,
            },
            session,
        )
    }
    pub(in super::super) fn capture_end<A: Admission>(
        &mut self,
        next: Id,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Id, Failure>> {
        let pattern = self.current_pattern(session)?;
        self.add(
            State::CaptureEnd {
                pattern,
                group: 0,
                next,
            },
            session,
        )
    }
    pub(in super::super) fn add_match<A: Admission>(
        &mut self,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Id, Failure>> {
        let pattern = self.current_pattern(session)?;
        self.add(State::Match { pattern }, session)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference::Account, Stop};
    #[test]
    fn registration_keeps_pattern_ids_captures_and_match_association() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut builder = Builder::new();
        for pattern in 0..2 {
            assert_eq!(
                builder.start_pattern(&mut session).unwrap().unwrap(),
                pattern
            );
            let start = builder
                .capture_start(Id::ZERO, &mut session)
                .unwrap()
                .unwrap();
            builder
                .capture_start(Id::ZERO, &mut session)
                .unwrap()
                .unwrap();
            assert_eq!(builder.captures.as_slice()[pattern as usize].len(), 1);
            let end = builder
                .capture_end(Id::ZERO, &mut session)
                .unwrap()
                .unwrap();
            let matched = builder.add_match(&mut session).unwrap().unwrap();
            builder.patch(start, end, &mut session).unwrap().unwrap();
            builder.patch(end, matched, &mut session).unwrap().unwrap();
            assert!(
                matches!(builder.states.as_slice()[matched.0 as usize], State::Match { pattern: got } if got == pattern)
            );
            assert_eq!(
                builder.finish_pattern(start, &mut session).unwrap(),
                pattern
            );
            assert_eq!(builder.starts.as_slice()[pattern as usize], start);
        }
        builder.clear(&mut session).unwrap();
        assert_eq!(builder.states.len(), 0);
        assert_eq!(builder.captures.len(), 0);
        assert_eq!(builder.start_pattern(&mut session).unwrap().unwrap(), 0);
    }
    #[test]
    fn semantic_state_failure_keeps_prior_capture_registration() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut builder = Builder::new();
        builder
            .set_size_limit(Some(0), &mut session)
            .unwrap()
            .unwrap();
        builder.start_pattern(&mut session).unwrap().unwrap();
        assert_eq!(
            builder.capture_start(Id::ZERO, &mut session).unwrap(),
            Err(Failure::SizeLimit(0))
        );
        assert_eq!(builder.captures.len(), 1);
        assert_eq!(builder.captures.as_slice()[0].len(), 1);
        assert_eq!(builder.states.len(), 1);
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
        let mut builder = Builder::new();
        for _ in 0..2 {
            builder.start_pattern(session)?.unwrap();
            let start = builder.capture_start(Id::ZERO, session)?.unwrap();
            builder.capture_end(Id::ZERO, session)?.unwrap();
            builder.add_match(session)?.unwrap();
            builder.finish_pattern(start, session)?;
        }
        builder.clear(session)
    }
    #[test]
    fn refusal_stops_pattern_and_capture_ownership_growth() {
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
