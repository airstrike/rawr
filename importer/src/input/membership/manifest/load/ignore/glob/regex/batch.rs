// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../LICENSE-regex-syntax-MIT.
//! regex-automata 0.4.14 meta::Builder::build_many AST retention boundary.
use super::{nfa, parser, translate, Regex};
use crate::input::membership::manifest::{session::Session, storage::Buffer, Error, Step};
use rawr_toml::document::Admission;

pub(super) struct Tree<'a> {
    pub(super) source: &'a [u8],
    pub(super) parsed: parser::Parsed,
}
pub(in super::super) struct Batch<'a>(Buffer<Tree<'a>>);
pub(in super::super) struct Failure {
    pub(super) pattern: usize,
    pub(super) cause: parser::Failure,
}
/// Retain every AST alongside every translated HIR until downstream compilation.
pub(in super::super) struct Translated<'a> {
    // build_many's local HIR vector drops before its AST vector.
    expressions: Buffer<translate::Expression>,
    trees: Batch<'a>,
}
pub(in super::super) struct TranslationFailure {
    pub(super) pattern: usize,
    pub(super) cause: translate::Failure,
}
impl Failure {
    pub(in super::super) fn render<A: Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<String> {
        use crate::input::membership::manifest::load::formatted;
        // Bounded tags/spans stand in for an upstream snippet
        // renderer. All source failure identity and precedence is retained.
        match &self.cause {
            parser::Failure::Flags(error) => formatted(
                format_args!(
                    "regex parse error in pattern {} at bytes {}..{}: {:?}",
                    self.pattern, error.span.start.offset, error.span.end.offset, error.kind,
                ),
                session,
            ),
            parser::Failure::Class(error) => formatted(
                format_args!(
                "regex parse error in pattern {} at bytes {}..{}: invalid character class range",
                self.pattern, error.span.start.offset, error.span.end.offset,
            ),
                session,
            ),
            parser::Failure::Nesting(error) => formatted(
                format_args!(
                    "regex parse error in pattern {} at bytes {}..{}: nesting limit {} exceeded",
                    self.pattern, error.span.start.offset, error.span.end.offset, error.limit,
                ),
                session,
            ),
        }
    }
}
impl TranslationFailure {
    pub(in super::super) fn render<A: Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<String> {
        self.cause.render(self.pattern, session)
    }
}
impl Translated<'_> {
    /// Complete the nonempty globset regex constructor while all AST/HIR
    /// owners (and Info's temporary HIR reference vector) are still retained.
    pub(in super::super) fn compile<A: Admission>(
        self,
        kind: nfa::Kind,
        session: &mut Session<'_, A>,
    ) -> Step<Result<nfa::Regex, nfa::Failure>> {
        nfa::Regex::new(self.expressions.as_slice(), kind, session)
    }

    pub(super) fn with_properties<A: Admission, T>(
        &self,
        session: &mut Session<'_, A>,
        operation: impl FnOnce(&translate::Properties, &mut Session<'_, A>) -> Step<T>,
    ) -> Step<T> {
        translate::Properties::with(self.expressions.as_slice(), session, operation)
    }
}
impl<'a> Batch<'a> {
    pub(in super::super) fn translate<A: Admission>(
        self,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Translated<'a>, TranslationFailure>> {
        session.check()?;
        let mut expressions = Buffer::new();
        for (pattern, tree) in self.trees().iter().enumerate() {
            session.items(1)?;
            let expression = match translate::bytes(tree.source, &tree.parsed, session)? {
                Ok(expression) => expression,
                Err(cause) => return Ok(Err(TranslationFailure { pattern, cause })),
            };
            // Finish translation before extending the HIR vector. All parsed
            // trees and preceding HIRs remain owned throughout this operation.
            expressions.push(expression, session)?;
        }
        Ok(Ok(Translated {
            expressions,
            trees: self,
        }))
    }
    pub(in super::super) fn parse<A: Admission>(
        patterns: &[&'a Regex],
        session: &mut Session<'_, A>,
    ) -> Step<Result<Self, Failure>> {
        Self::borrowed(patterns.iter().map(|source| source.bytes()), session)
    }

    pub(in super::super) fn parse_text<A: Admission>(
        pattern: &'a str,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Self, Failure>> {
        Self::borrowed(std::iter::once(pattern.as_bytes()), session)
    }

    pub(in super::super) fn parse_texts<A: Admission>(
        patterns: &'a [String],
        session: &mut Session<'_, A>,
    ) -> Step<Result<Self, Failure>> {
        Self::borrowed(patterns.iter().map(|source| source.as_bytes()), session)
    }

    fn borrowed<A: Admission>(
        patterns: impl ExactSizeIterator<Item = &'a [u8]>,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Self, Failure>> {
        session.check()?;
        // Pinned PatternID::iter checks the entire length before parsing.
        // Refuse an unrepresentable internal batch instead of reaching its panic.
        if patterns.len() > i32::MAX as usize {
            return session.fail(Error::Overflow);
        }
        let mut trees = Buffer::new();
        for (pattern, source) in patterns.enumerate() {
            session.items(1)?;
            let parsed = match super::tree(source, session)? {
                Ok(parsed) => parsed,
                Err(cause) => return Ok(Err(Failure { pattern, cause })),
            };
            // Source order: finish parsing before growing the retained vector.
            trees.push(Tree { source, parsed }, session)?;
        }
        Ok(Ok(Self(trees)))
    }
    pub(super) fn trees(&self) -> &[Tree<'a>] {
        self.0.as_slice()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference::Account, Stop};

    fn emitted(glob: &str) -> Regex {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let parsed = super::super::super::parse(glob, &mut session)
            .unwrap()
            .ok()
            .unwrap();
        parsed.regex(&mut session).unwrap()
    }

    #[test]
    fn translation_retains_all_parsed_trees_and_hirs_in_order() {
        let first = emitted("{a,b}*");
        let second = emitted("[0-9]/é");
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let batch = Batch::parse(&[&first, &second], &mut session)
            .unwrap()
            .ok()
            .unwrap();
        let translated = batch.translate(&mut session).unwrap().ok().unwrap();
        assert_eq!(translated.expressions.len(), 2);
        translated
            .with_properties(&mut session, |properties, _| {
                assert_eq!(properties.pattern_len(), 2);
                assert!(properties.anchored_start());
                Ok(())
            })
            .unwrap();
        assert_eq!(translated.trees.trees()[0].source, first.bytes());
        assert_eq!(translated.trees.trees()[1].source, second.bytes());
        let empty = Batch::parse(&[], &mut session)
            .unwrap()
            .ok()
            .unwrap()
            .translate(&mut session)
            .unwrap()
            .ok()
            .unwrap();
        assert_eq!(empty.expressions.len(), 0);
        assert!(empty.trees.trees().is_empty());
    }

    #[test]
    fn batch_retains_order_and_empty_set() {
        let first = emitted("a*");
        let second = emitted("[a-z]");
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let batch = Batch::parse(&[&first, &second], &mut session)
            .unwrap()
            .ok()
            .unwrap();
        assert_eq!(batch.trees().len(), 2);
        assert!(std::ptr::eq(batch.trees()[0].source, first.bytes()));
        assert!(std::ptr::eq(batch.trees()[1].source, second.bytes()));
        assert!(Batch::parse(&[], &mut session)
            .unwrap()
            .ok()
            .unwrap()
            .trees()
            .is_empty());
    }

    #[test]
    fn first_failed_pattern_stops_batch() {
        let first = emitted("a");
        let deep = emitted(&format!("{}a{}", "{".repeat(251), "}".repeat(251)));
        // Deliberately invalid internal bytes must never be inspected after failure.
        let mut text = super::super::Text::new();
        text.append(&[0xff], &mut Session::new(&mut Account))
            .unwrap();
        let later = Regex(text);
        let result =
            Batch::parse(&[&first, &deep, &later], &mut Session::new(&mut Account)).unwrap();
        let Err(failure) = result else {
            panic!("expected nesting failure")
        };
        assert_eq!(failure.pattern, 1);
        assert!(matches!(failure.cause, parser::Failure::Nesting(_)));
    }

    struct Count {
        calls: usize,
        refuse: usize,
    }
    impl Admission for Count {
        type Error = ();
        fn admit(&mut self, _: rawr_toml::document::Request) -> Result<(), ()> {
            let index = self.calls;
            self.calls += 1;
            if index == self.refuse {
                Err(())
            } else {
                Ok(())
            }
        }
    }
    #[test]
    fn property_refusal_never_enters_next_construction_stage() {
        let first = emitted("a*");
        let second = emitted("[a-z]");
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let translated = Batch::parse(&[&first, &second], &mut session)
            .unwrap()
            .ok()
            .unwrap()
            .translate(&mut session)
            .unwrap()
            .ok()
            .unwrap();
        let mut count = Count {
            calls: 0,
            refuse: usize::MAX,
        };
        translated
            .with_properties(&mut Session::new(&mut count), |_, _| Ok(()))
            .unwrap();
        for refuse in 0..count.calls {
            let mut count = Count { calls: 0, refuse };
            let mut session = Session::new(&mut count);
            let mut entered = false;
            assert!(matches!(
                translated.with_properties(&mut session, |_, _| {
                    entered = true;
                    Ok(())
                }),
                Err(Stop::Refused)
            ));
            assert!(!entered);
            assert!(matches!(session.check(), Err(Stop::Refused)));
            assert_eq!(count.calls, refuse + 1);
        }
    }
    #[test]
    fn batch_refusal_is_terminal() {
        let first = emitted("a*");
        let second = emitted("[a-z]");
        let mut count = Count {
            calls: 0,
            refuse: usize::MAX,
        };
        assert!(
            Batch::parse(&[&first, &second], &mut Session::new(&mut count))
                .unwrap()
                .is_ok()
        );
        for refuse in 0..count.calls {
            let mut count = Count { calls: 0, refuse };
            let mut session = Session::new(&mut count);
            assert!(matches!(
                Batch::parse(&[&first, &second], &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(session.check(), Err(Stop::Refused)));
            assert_eq!(count.calls, refuse + 1);
        }
    }
}
