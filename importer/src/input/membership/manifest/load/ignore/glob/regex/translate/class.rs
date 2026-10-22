// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../LICENSE-regex-syntax-MIT.
//! Reached bracketed-byte-class translation, before Hir::class normalization.
use super::super::{class as ast, cursor::Span, primitive::Primitive};
use super::{bytes, Flags, Scalar};
use crate::input::membership::manifest::{
    load::formatted, session::Session, storage::Buffer, Error, Step,
};
use rawr_toml::document::Admission;

pub(super) struct Failure {
    pub(super) pattern: String,
    pub(super) span: Span,
}
/// Retain both translator and AST visitor continuation capacities across classes.
pub(super) struct Classes<'a> {
    values: Buffer<bytes::Class>,
    parents: Buffer<&'a [ast::Item]>,
}
impl<'a> Classes<'a> {
    pub(super) fn expression<A: Admission>(
        &mut self,
        class: &'a ast::Class,
        flags: Flags,
        pattern: &str,
        session: &mut Session<'_, A>,
    ) -> Step<Result<super::hir::Expr, Failure>> {
        match self.translate(class, flags, pattern, session)? {
            Ok(class) => super::hir::Expr::bytes(class, session).map(Ok),
            Err(error) => Ok(Err(error)),
        }
    }
    pub(super) fn new() -> Self {
        Self {
            values: Buffer::new(),
            parents: Buffer::new(),
        }
    }
    pub(super) fn translate<A: Admission>(
        &mut self,
        class: &'a ast::Class,
        flags: Flags,
        pattern: &str,
        session: &mut Session<'_, A>,
    ) -> Step<Result<bytes::Class, Failure>> {
        session.check()?;
        // The emitter always puts (?-u) before classes and emits no case flag.
        if flags.unicode()
            || flags.case_insensitive()
            || self.values.len() != 0
            || self.parents.len() != 0
        {
            return session.fail(Error::State);
        }
        self.values.push(bytes::Class::empty(), session)?;
        match &class.set {
            ast::Set::Empty(_) => {}
            ast::Set::Single(item) => {
                if let Err(error) = self.item(item, flags, pattern, session)? {
                    return Ok(Err(error));
                }
            }
            ast::Set::Union { items, .. } => {
                let mut remaining = items.as_slice();
                while let Some((item, tail)) = remaining.split_first() {
                    // Pinned class visitor pushes the union continuation before
                    // invoking the child's post-visit translator operation.
                    self.parents.push(tail, session)?;
                    if let Err(error) = self.item(item, flags, pattern, session)? {
                        return Ok(Err(error));
                    }
                    let Some(tail) = self.parents.pop() else {
                        return session.fail(Error::State);
                    };
                    remaining = tail;
                }
            }
        }
        let Some(mut value) = self.values.pop() else {
            return session.fail(Error::State);
        };
        if class.negated {
            value.negate(session)?;
        }
        Ok(Ok(value))
    }
    fn item<A: Admission>(
        &mut self,
        item: &ast::Item,
        flags: Flags,
        pattern: &str,
        session: &mut Session<'_, A>,
    ) -> Step<Result<(), Failure>> {
        let Some(mut value) = self.values.pop() else {
            return session.fail(Error::State);
        };
        let (start, end) = match item {
            ast::Item::Literal(literal) => {
                let byte = match literal_byte(literal, flags, pattern, session)? {
                    Ok(byte) => byte,
                    Err(error) => return Ok(Err(error)),
                };
                (byte, byte)
            }
            ast::Item::Range { start, end, .. } => {
                let start = match literal_byte(start, flags, pattern, session)? {
                    Ok(byte) => byte,
                    Err(error) => return Ok(Err(error)),
                };
                let end = match literal_byte(end, flags, pattern, session)? {
                    Ok(byte) => byte,
                    Err(error) => return Ok(Err(error)),
                };
                (start, end)
            }
        };
        value.push(bytes::Range::new(start, end), session)?;
        self.values.push(value, session)?;
        Ok(Ok(()))
    }
}
fn literal_byte<A: Admission>(
    literal: &Primitive,
    flags: Flags,
    pattern: &str,
    session: &mut Session<'_, A>,
) -> Step<Result<u8, Failure>> {
    match flags.scalar(literal, session)? {
        Scalar::Byte(byte) => Ok(Ok(byte)),
        Scalar::Char(c) if c.is_ascii() => Ok(Ok(c as u8)),
        Scalar::Char(_) => Ok(Err(Failure {
            pattern: formatted(format_args!("{pattern}"), session)?,
            span: literal.span,
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::{cursor::Cursor, flags, primitive::Scratch};
    use super::*;
    use crate::input::membership::manifest::reference::Account;
    fn parse(pattern: &str) -> ast::Class {
        ast::parse(
            &mut Cursor::start(pattern),
            &mut Scratch::new(),
            &mut ast::Stack::new(),
            &mut Session::new(&mut Account),
        )
        .unwrap()
        .ok()
        .unwrap()
    }
    fn flags() -> Flags {
        let mut current = Flags::initial();
        let ast = flags::Flags::parse(&mut Cursor::start("-u)"), &mut Session::new(&mut Account))
            .unwrap()
            .ok()
            .unwrap();
        current.set(&ast, &mut Session::new(&mut Account)).unwrap();
        current
    }
    #[test]
    fn classes_preserve_byte_escapes_and_negation() {
        let first = parse("[\\xc3\\xa9a-c]");
        let second = parse("[^/]");
        let mut work = Classes::new();
        let first = work
            .translate(
                &first,
                flags(),
                "[\\xc3\\xa9a-c]",
                &mut Session::new(&mut Account),
            )
            .unwrap()
            .ok()
            .unwrap();
        for b in 0..=255 {
            assert_eq!(first.contains(b), matches!(b, 0xc3 | 0xa9 | b'a'..=b'c'));
        }
        let second = work
            .translate(&second, flags(), "[^/]", &mut Session::new(&mut Account))
            .unwrap()
            .ok()
            .unwrap();
        for b in 0..=255 {
            assert_eq!(second.contains(b), b != b'/');
        }
    }

    #[test]
    fn parsed_class_reaches_normalized_expression() {
        let ast = parse("[a]");
        let mut work = Classes::new();
        let expr = work
            .expression(&ast, flags(), "[a]", &mut Session::new(&mut Account))
            .unwrap()
            .ok()
            .unwrap();
        let super::super::hir::Kind::Literal(literal) = expr.kind else {
            panic!("singleton normalization")
        };
        assert_eq!(literal.bytes(), b"a");
        assert!(expr.properties.get().literal);
    }
    #[test]
    fn non_byte_range_endpoint_reports_first_literal_span() {
        let source = "[é-ê]";
        let ast = parse(source);
        let Err(error) = Classes::new()
            .translate(&ast, flags(), source, &mut Session::new(&mut Account))
            .unwrap()
        else {
            panic!("UnicodeNotAllowed")
        };
        assert_eq!(error.pattern, source);
        assert_eq!(error.span.start.offset, 1);
        assert_eq!(error.span.end.offset, 3);
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
    fn class_translation_refuses_before_each_reached_operation() {
        use crate::input::membership::manifest::Stop;
        for pattern in ["[^a-cx]", "[aé]"] {
            let ast = parse(pattern);
            let flags = flags();
            let mut count = Count {
                calls: 0,
                refuse: usize::MAX,
            };
            let _ = Classes::new()
                .expression(&ast, flags, pattern, &mut Session::new(&mut count))
                .unwrap();
            for refuse in 0..count.calls {
                let mut count = Count { calls: 0, refuse };
                let mut session = Session::new(&mut count);
                let mut work = Classes::new();
                assert!(matches!(
                    work.expression(&ast, flags, pattern, &mut session),
                    Err(Stop::Refused)
                ));
                assert!(matches!(
                    work.expression(&ast, flags, pattern, &mut session),
                    Err(Stop::Refused)
                ));
                assert_eq!(count.calls, refuse + 1);
            }
        }
    }
}
