// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../LICENSE-regex-syntax-MIT.
//! Pinned compile batch configuration, anchoring and top-level alternation.
use super::super::super::translate::Expression;
use super::super::builder::Id;
use super::*;

pub(in super::super) struct Starts {
    pub(in super::super) anchored: Id,
    pub(in super::super) unanchored: Id,
}
impl Compiler {
    /// Core fixes UTF-8-empty=false and LF look matching; reverse construction
    /// disables captures. The enclosing finalizer must preserve these settings.
    pub(in super::super) fn many<A: Admission>(
        &mut self,
        expressions: &[Expression],
        size_limit: Option<usize>,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Starts, Failure>> {
        session.items(1)?;
        if expressions.len() > i32::MAX as usize {
            return Ok(Err(Failure::TooManyPatterns(expressions.len())));
        }
        self.builder.clear(session)?;
        if let Err(e) = self.builder.set_size_limit(size_limit, session)? {
            return Ok(Err(e));
        }
        let mut all_anchored = true;
        let mut scan = expressions.iter();
        loop {
            session.items(1)?;
            let Some(expression) = scan.next() else { break };
            if !expression.view().anchored(self.reverse, session)? {
                all_anchored = false;
                break;
            }
        }
        let prefix = if all_anchored {
            self.builder.empty(session)?
        } else {
            let any = Expression::any_byte(session)?;
            let minimum = any.view().minimum_len(session)?;
            let pending = match self
                .builder
                .begin_repeat(0, None, false, minimum, session)?
            {
                Ok(r) => r,
                Err(e) => return Ok(Err(e)),
            };
            let child = match self.compile(any.view(), session)? {
                Ok(f) => f,
                Err(e) => return Ok(Err(e)),
            };
            let result = self.builder.finish_repeat(pending, child, session)?;
            drop(any);
            result
        };
        let prefix = match prefix {
            Ok(f) => f,
            Err(e) => return Ok(Err(e)),
        };
        let compiled = match self.patterns(expressions, session)? {
            Ok(f) => f,
            Err(e) => return Ok(Err(e)),
        };
        if let Err(e) = self.builder.patch(prefix.end, compiled.start, session)? {
            return Ok(Err(e));
        }
        Ok(Ok(Starts {
            anchored: compiled.start,
            unanchored: prefix.start,
        }))
    }
    fn patterns<A: Admission>(
        &mut self,
        expressions: &[Expression],
        session: &mut Session<'_, A>,
    ) -> Step<Result<Fragment, Failure>> {
        let mut expressions = expressions.iter();
        session.items(1)?;
        let Some(first) = expressions.next() else {
            return self.builder.fail(session);
        };
        let first = match self.pattern(first.view(), session)? {
            Ok(f) => f,
            Err(e) => return Ok(Err(e)),
        };
        session.items(1)?;
        let Some(second) = expressions.next() else {
            return Ok(Ok(first));
        };
        let second = match self.pattern(second.view(), session)? {
            Ok(f) => f,
            Err(e) => return Ok(Err(e)),
        };
        let combined = match self.builder.alternate_pair(first, second, session)? {
            Ok(f) => f,
            Err(e) => return Ok(Err(e)),
        };
        loop {
            session.items(1)?;
            let Some(expression) = expressions.next() else {
                break;
            };
            let next = match self.pattern(expression.view(), session)? {
                Ok(f) => f,
                Err(e) => return Ok(Err(e)),
            };
            if let Err(e) = self.builder.alternate_next(&combined, next, session)? {
                return Ok(Err(e));
            }
        }
        Ok(Ok(combined))
    }
}
