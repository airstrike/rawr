//! Focused checks of the retained raw API; document admission is separate.

use crate::{decoder, lexer, parser, ParseError, Source};

#[test]
fn lexer_keeps_native_utf8_spans_and_terminal_eof() {
    let input = "\u{feff}name = \"é\"\n";
    let source = Source::new(input);
    let tokens: Vec<_> = source.lex().collect();

    assert_eq!(tokens[0].span().start(), 3);
    assert_eq!(source.get(tokens[0]).unwrap().as_str(), "name");
    let string = tokens
        .iter()
        .find(|token| token.kind() == lexer::TokenKind::BasicString)
        .unwrap();
    assert_eq!(source.get(string).unwrap().as_str(), "\"é\"");
    let eof = tokens.last().unwrap();
    assert_eq!(eof.kind(), lexer::TokenKind::Eof);
    assert_eq!(eof.span().start(), input.len());
    assert_eq!(eof.span().end(), input.len());
}

#[test]
fn decoded_key_identity_ignores_escape_spelling() {
    for input in ["name", r#""na\u006de""#] {
        let source = Source::new(input);
        let token = source.lex().next().unwrap();
        let mut decoded = String::new();
        let mut error = None;
        source
            .get(token)
            .unwrap()
            .decode_key(&mut decoded, &mut error);
        assert_eq!(decoded, "name");
        assert_eq!(error, None);
    }
}

#[test]
fn parser_reports_recovery_and_continues_to_later_scalar() {
    // Adjacent quoted values stay separate; raw atoms can share a scalar span.
    let source = Source::new("a = [\"1\" \"2\"]\nb = 3\n");
    let tokens: Vec<_> = source.lex().collect();
    let mut scalars = Vec::new();
    let mut events = |event: parser::Event| {
        if event.kind() == parser::EventKind::Scalar {
            scalars.push(source.get(event).unwrap().as_str());
        }
    };
    let mut receiver = parser::ValidateWhitespace::new(&mut events, source);
    let mut error = None;
    parser::parse_document(&tokens, &mut receiver, &mut error);

    assert_eq!(
        error.unwrap().description(),
        "missing comma between array elements"
    );
    assert_eq!(scalars.last(), Some(&"3"));
}

#[test]
fn guard_calls_inner_open_before_rejecting_depth() {
    let source = Source::new("a = [[[0]]]\nb = 1\n");
    let tokens: Vec<_> = source.lex().collect();
    let mut opens = 0;
    let mut events = |event: parser::Event| {
        if event.kind() == parser::EventKind::ArrayOpen {
            opens += 1;
        }
    };
    let mut whitespace = parser::ValidateWhitespace::new(&mut events, source);
    let mut guard = parser::RecursionGuard::new(&mut whitespace, 1);
    let mut error = None;
    parser::parse_document(&tokens, &mut guard, &mut error);

    assert_eq!(opens, 2);
    assert_eq!(
        error.unwrap().description(),
        "cannot recurse further; max recursion depth met"
    );
}

#[test]
fn raw_numbers_retain_normalized_spelling_without_machine_conversion() {
    for (input, expected) in [
        ("1_000", "1000"),
        (
            "99999999999999999999999999999999999999999999999999999999",
            "99999999999999999999999999999999999999999999999999999999",
        ),
    ] {
        let source = Source::new(input);
        let token = source.lex().next().unwrap();
        let mut decoded = String::new();
        let mut error = None;
        let kind = source
            .get(token)
            .unwrap()
            .decode_scalar(&mut decoded, &mut error);
        assert!(matches!(kind, decoder::ScalarKind::Integer(_)));
        assert_eq!(decoded, expected);
        assert_eq!(error, None);
    }
}

#[test]
fn builder_refusal_does_not_cancel_raw_decoding() {
    struct Refused {
        writes: usize,
    }

    impl<'input> decoder::StringBuilder<'input> for Refused {
        fn clear(&mut self) {}

        fn push_str(&mut self, _append: &'input str) -> bool {
            self.writes += 1;
            false
        }

        fn push_char(&mut self, _append: char) -> bool {
            self.writes += 1;
            false
        }
    }

    let source = Source::new(r#""a\nb\tc""#);
    let token = source.lex().next().unwrap();
    let mut output = Refused { writes: 0 };
    let mut first: Option<ParseError> = None;
    let kind = source
        .get(token)
        .unwrap()
        .decode_scalar(&mut output, &mut first);

    assert_eq!(kind, decoder::ScalarKind::String);
    assert!(output.writes > 1);
    assert_eq!(
        first.unwrap().description(),
        "could not allocate for string"
    );
}
