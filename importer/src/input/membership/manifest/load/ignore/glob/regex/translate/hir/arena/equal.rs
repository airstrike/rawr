// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../../LICENSE-regex-syntax-MIT.
//! Source structural equality, with explicit admitted continuations.
use super::super::Look;
use super::{Arena, Buffer, Expr, Id, Kind, Properties, Session, Step};
use rawr_toml::document::Admission;

enum Frame<'a> {
    Expr(&'a Expr, &'a Expr),
    Properties(&'a Properties, &'a Properties),
    Children(&'a [Id], &'a [Id]),
}
impl Arena {
    pub(super) fn equal<A: Admission>(
        &self,
        left: Id,
        right: Id,
        session: &mut Session<'_, A>,
    ) -> Step<bool> {
        let mut pending = Buffer::new();
        pending.push(
            Frame::Expr(self.get(left, session)?, self.get(right, session)?),
            session,
        )?;
        while let Some(frame) = pending.pop() {
            session.items(1)?;
            match frame {
                Frame::Properties(a, b) => {
                    if a.get() != b.get() {
                        return Ok(false);
                    }
                }
                Frame::Children(a, b) => {
                    if a.len() != b.len() {
                        return Ok(false);
                    }
                    if let (Some((a, rest_a)), Some((b, rest_b))) =
                        (a.split_first(), b.split_first())
                    {
                        pending.push(Frame::Children(rest_a, rest_b), session)?;
                        pending.push(
                            Frame::Expr(self.get(*a, session)?, self.get(*b, session)?),
                            session,
                        )?;
                    }
                }
                Frame::Expr(a, b) => {
                    // Derived Hir equality compares kind (and its children)
                    // before the parent's scalar properties.
                    pending.push(Frame::Properties(&a.properties, &b.properties), session)?;
                    match (&a.kind, &b.kind) {
                        (Kind::Empty, Kind::Empty) => {}
                        (Kind::Literal(a), Kind::Literal(b)) => {
                            if a.bytes().len() != b.bytes().len() {
                                return Ok(false);
                            }
                            session.items(a.bytes().len())?;
                            if a.bytes() != b.bytes() {
                                return Ok(false);
                            }
                        }
                        (Kind::Bytes(a), Kind::Bytes(b)) => {
                            if !a.equal(b, session)? {
                                return Ok(false);
                            }
                        }
                        (Kind::Unicode(a), Kind::Unicode(b)) => {
                            if !a.equal(b, session)? {
                                return Ok(false);
                            }
                        }
                        (Kind::Look(Look::Start), Kind::Look(Look::Start))
                        | (Kind::Look(Look::End), Kind::Look(Look::End)) => {}
                        (Kind::Concat(a), Kind::Concat(b))
                        | (Kind::Alternation(a), Kind::Alternation(b)) => {
                            pending.push(Frame::Children(a.as_slice(), b.as_slice()), session)?;
                        }
                        (
                            Kind::Repetition {
                                min: a,
                                max: b,
                                greedy: c,
                                child: d,
                            },
                            Kind::Repetition {
                                min: e,
                                max: f,
                                greedy: g,
                                child: h,
                            },
                        ) => {
                            if (a, b, c) != (e, f, g) {
                                return Ok(false);
                            }
                            pending.push(
                                Frame::Expr(self.get(*d, session)?, self.get(*h, session)?),
                                session,
                            )?;
                        }
                        _ => return Ok(false),
                    }
                }
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::Literal;
    use super::*;
    use crate::input::membership::manifest::reference::Account;

    #[test]
    fn equality_is_structural_and_handles_deep_separate_owners() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut arena = Arena::new();
        let mut ids = Vec::new();
        for value in [b"a", b"a", b"b"] {
            let mut expr =
                Expr::literal(Literal::copy(value, &mut session).unwrap(), &mut session).unwrap();
            for _ in 0..1000 {
                expr = arena.repetition(expr, 1, None, true, &mut session).unwrap();
            }
            ids.push(arena.retain(expr, &mut session).unwrap());
        }
        assert!(arena.equal(ids[0], ids[1], &mut session).unwrap());
        assert!(!arena.equal(ids[0], ids[2], &mut session).unwrap());
        for id in ids {
            arena.discard(id, &mut session).unwrap();
        }
        assert!(arena.0.as_slice().iter().all(Option::is_none));
    }
}
