//! MultiStrategyBuilder's regex-set branch and retained PatternSet queries.
use super::{compile, regex::nfa, Admission, Buffer, Error, Failure, Session, Step};

pub(in super::super) use super::multi::Builder;

pub(in super::super) struct Strategy {
    matcher: nfa::Regex,
    map: Buffer<usize>,
    scratch: Scratch,
}

// This replaces the non-shared source pool's dispatch only. Its captured scalar
// and separately lazy PatternSet remain distinct from the Regex's Core cache.
struct Scratch {
    pattern_len: usize,
    value: Option<nfa::Set>,
}

impl Builder {
    pub(in super::super) fn build<A: Admission>(
        self,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Strategy, Failure>> {
        let matcher = match compile::set(self.literals, session)? {
            Ok(matcher) => matcher,
            Err(error) => return Ok(Err(error)),
        };
        session.items(1)?;
        let pattern_len = matcher.pattern_len();
        let scratch = Scratch {
            pattern_len,
            value: None,
        };
        Ok(Ok(Strategy {
            matcher,
            map: self.map,
            scratch,
        }))
    }
}

impl Strategy {
    pub(in super::super) fn is_match<A: Admission>(
        &mut self,
        path: &[u8],
        session: &mut Session<'_, A>,
    ) -> Step<bool> {
        self.matcher.is_match(path, session)
    }

    pub(in super::super) fn matches_into<A: Admission>(
        &mut self,
        path: &[u8],
        matches: &mut Buffer<usize>,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        let patterns = self.scratch.get(session)?;
        patterns.clear(session)?;
        self.matcher.overlapping(path, patterns, session)?;
        let mut cursor = 0;
        while let Some(pattern) = patterns.next(&mut cursor, session)? {
            session.items(1)?;
            let Some(&index) = self.map.as_slice().get(pattern as usize) else {
                return session.fail(Error::State);
            };
            matches.push(index, session)?;
        }
        Ok(())
    }
}

impl Scratch {
    fn get<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Step<&mut nfa::Set> {
        session.items(1)?;
        if self.value.is_none() {
            let value = nfa::Set::new(self.pattern_len, session)?;
            self.value = Some(value);
        }
        match self.value.as_mut() {
            Some(value) => Ok(value),
            None => session.fail(Error::State),
        }
    }
}
