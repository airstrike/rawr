// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../LICENSE-regex-syntax-MIT.
//! Reached Compiler::new ownership and one pattern's implicit capture wrapper.
use super::super::builder::{Id, Transition};
use super::*;

/// Only the two empty state transition vectors are allocated by RangeTrie::new.
/// Its other scratch/free vectors remain empty on Core's shrink=false paths.
pub(super) struct Initial {
    _states: Buffer<Buffer<Transition>>,
}
impl Initial {
    pub(super) fn new<A: Admission>(session: &mut Session<'_, A>) -> Step<Self> {
        let mut states = Buffer::new();
        states.push(Buffer::new(), session)?; // final state
        states.push(Buffer::new(), session)?; // root state
        Ok(Self { _states: states })
    }
}
impl Compiler {
    pub(in super::super) fn pattern<A: Admission>(
        &mut self,
        expression: View<'_>,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Fragment, Failure>> {
        if let Err(e) = self.builder.start_pattern(session)? {
            return Ok(Err(e));
        }
        let start = if self.reverse {
            None
        } else {
            Some(match self.builder.capture_start(Id::ZERO, session)? {
                Ok(id) => id,
                Err(e) => return Ok(Err(e)),
            })
        };
        let mut compiled = match self.compile(expression, session)? {
            Ok(f) => f,
            Err(e) => return Ok(Err(e)),
        };
        if let Some(start) = start {
            let end = match self.builder.capture_end(Id::ZERO, session)? {
                Ok(id) => id,
                Err(e) => return Ok(Err(e)),
            };
            if let Err(e) = self.builder.patch(start, compiled.start, session)? {
                return Ok(Err(e));
            }
            if let Err(e) = self.builder.patch(compiled.end, end, session)? {
                return Ok(Err(e));
            }
            compiled = Fragment { start, end };
        }
        let matched = match self.builder.add_match(session)? {
            Ok(id) => id,
            Err(e) => return Ok(Err(e)),
        };
        if let Err(e) = self.builder.patch(compiled.end, matched, session)? {
            return Ok(Err(e));
        }
        self.builder.finish_pattern(compiled.start, session)?;
        Ok(Ok(Fragment {
            start: compiled.start,
            end: matched,
        }))
    }
}
