// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../../LICENSE-regex-syntax-MIT.
//! Pinned Hir::concat consuming flattening and adjacent-literal normalization.
use super::super::Literal;
use super::{Arena, Buffer, Error, Expr, Kind, Properties, Session, Step};
use rawr_toml::document::Admission;

impl Arena {
    pub(in super::super) fn concat<A: Admission>(
        &mut self,
        children: Buffer<Expr>,
        session: &mut Session<'_, A>,
    ) -> Step<Expr> {
        session.check()?;
        let mut output = Buffer::new();
        let mut literal = None;
        for child in children.into_iter() {
            session.items(1)?;
            let (kind, properties) = child.into_parts(session)?;
            match kind {
                Kind::Literal(bytes) => append(&mut literal, bytes.bytes(), session)?,
                Kind::Concat(children) => {
                    for child in children.into_iter() {
                        let child = self.take(child, session)?;
                        let (kind, properties) = child.into_parts(session)?;
                        match kind {
                            Kind::Literal(bytes) => append(&mut literal, bytes.bytes(), session)?,
                            kind => {
                                flush(&mut literal, &mut output, session)?;
                                output.push(Expr { kind, properties }, session)?;
                            }
                        }
                    }
                }
                Kind::Empty => {}
                kind => {
                    flush(&mut literal, &mut output, session)?;
                    output.push(Expr { kind, properties }, session)?;
                }
            }
        }
        flush(&mut literal, &mut output, session)?;
        if output.len() == 0 {
            return Expr::empty(session);
        }
        if output.len() == 1 {
            return match output.pop() {
                Some(expr) => Ok(expr),
                None => session.fail(Error::State),
            };
        }
        let properties = Properties::concat(output.as_slice(), session)?;
        let mut children = Buffer::new();
        for expr in output.into_iter() {
            let id = self.retain(expr, session)?;
            children.push(id, session)?;
        }
        Ok(Expr {
            kind: Kind::Concat(children),
            properties,
        })
    }
}
fn append<A: Admission>(
    prior: &mut Option<Buffer<u8>>,
    bytes: &[u8],
    session: &mut Session<'_, A>,
) -> Step<()> {
    let output = prior.get_or_insert_with(Buffer::new);
    for &byte in bytes {
        output.push(byte, session)?;
    }
    Ok(())
}
fn flush<A: Admission>(
    prior: &mut Option<Buffer<u8>>,
    output: &mut Buffer<Expr>,
    session: &mut Session<'_, A>,
) -> Step<()> {
    if let Some(bytes) = prior.take() {
        let literal = Literal::copy(bytes.as_slice(), session)?;
        drop(bytes);
        let expr = Expr::literal(literal, session)?;
        output.push(expr, session)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::super::Look;
    use super::*;
    use crate::input::membership::manifest::{reference::Account, Stop};
    fn input(
        children: impl IntoIterator<Item = Expr>,
        session: &mut Session<'_, Account>,
    ) -> Buffer<Expr> {
        let mut result = Buffer::new();
        for expr in children {
            result.push(expr, session).unwrap();
        }
        result
    }
    fn literal(bytes: &[u8], session: &mut Session<'_, Account>) -> Expr {
        Expr::literal(Literal::copy(bytes, session).unwrap(), session).unwrap()
    }
    #[test]
    fn adjacent_literals_recombine_utf8_and_empty_children_disappear() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut arena = Arena::new();
        let children = input(
            [
                literal(&[0xc3], &mut session),
                Expr::empty(&mut session).unwrap(),
                literal(&[0xa9], &mut session),
            ],
            &mut session,
        );
        let expr = arena.concat(children, &mut session).unwrap();
        assert!(expr.properties.get().utf8);
        let Kind::Literal(bytes) = expr.kind else {
            panic!("combined literal")
        };
        assert_eq!(bytes.bytes(), "é".as_bytes());
    }
    #[test]
    fn nested_concat_flattens_then_zero_repeat_releases_all_children() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut arena = Arena::new();
        let nested = input(
            [
                literal(b"a", &mut session),
                Expr::look(Look::End, &mut session).unwrap(),
            ],
            &mut session,
        );
        let nested = arena.concat(nested, &mut session).unwrap();
        let outer = input(
            [
                Expr::look(Look::Start, &mut session).unwrap(),
                literal(b"x", &mut session),
                nested,
            ],
            &mut session,
        );
        let expr = arena.concat(outer, &mut session).unwrap();
        let Kind::Concat(children) = &expr.kind else {
            panic!("concat")
        };
        assert_eq!(children.len(), 3);
        let middle = arena.get(children.as_slice()[1], &mut session).unwrap();
        let Kind::Literal(bytes) = &middle.kind else {
            panic!("middle literal")
        };
        assert_eq!(bytes.bytes(), b"xa");
        let empty = arena
            .repetition(expr, 0, Some(0), true, &mut session)
            .unwrap();
        assert!(matches!(empty.kind, Kind::Empty));
        assert!(arena.0.as_slice().iter().all(Option::is_none));
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
    fn concat_refusal_stops_copies_and_property_construction() {
        fn source() -> Buffer<Expr> {
            let mut account = Account;
            let mut session = Session::new(&mut account);
            input(
                [
                    literal(b"a", &mut session),
                    literal(b"b", &mut session),
                    Expr::look(Look::End, &mut session).unwrap(),
                ],
                &mut session,
            )
        }
        let mut count = Count {
            calls: 0,
            refuse: usize::MAX,
        };
        Arena::new()
            .concat(source(), &mut Session::new(&mut count))
            .unwrap();
        for refuse in 0..count.calls {
            let mut count = Count { calls: 0, refuse };
            let mut session = Session::new(&mut count);
            assert!(matches!(
                Arena::new().concat(source(), &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(session.check(), Err(Stop::Refused)));
            assert_eq!(count.calls, refuse + 1);
        }
    }
}
