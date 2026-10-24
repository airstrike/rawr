use super::{escape_environment, escape_path, Environment, Error, Event, Record, Syntax, Tokens};

fn reference(text: &str) -> Result<Vec<Event>, Syntax> {
    if text.ends_with("\\\n") {
        return Err(Syntax::TrailingLineEscape);
    }
    let normalized = text.replace("\\\n", "");
    let rule = normalized.lines().next().ok_or(Syntax::MissingRule)?;
    // The target ends at a colon before whitespace or the line's end.
    let colon = rule
        .char_indices()
        .find(|&(at, character)| {
            character == ':'
                && rule[at + 1..]
                    .chars()
                    .next()
                    .is_none_or(char::is_whitespace)
        })
        .map(|(at, _)| at)
        .ok_or(Syntax::MissingRule)?;
    let dependencies = &rule[colon + 1..];
    let mut events = Vec::new();
    let mut token = String::new();
    let mut escaped = false;
    for character in dependencies.chars() {
        if escaped {
            token.push(character);
            events.push(Event::Character(character));
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character.is_whitespace() {
            if !token.is_empty() {
                events.push(Event::Boundary);
                token.clear();
            }
        } else {
            token.push(character);
            events.push(Event::Character(character));
        }
    }
    if escaped {
        return Err(Syntax::TrailingEscape);
    }
    if !token.is_empty() {
        events.push(Event::Boundary);
    }
    Ok(events)
}

fn decoded(text: &str) -> Result<Vec<String>, Syntax> {
    let mut values = Vec::new();
    let mut value = String::new();
    for event in Tokens::raw(text)? {
        match event? {
            Event::Character(character) => value.push(character),
            Event::Boundary => values.push(std::mem::take(&mut value)),
        }
    }
    Ok(values)
}

#[test]
fn both_character_sources_match_the_original_rule_loop() {
    let alphabet = ['a', '\\', '\n', '\r', ' ', ':'];
    for length in 0..=5 {
        for mut encoded in 0..alphabet.len().pow(length) {
            let mut text = String::from("target:");
            for _ in 0..length {
                text.push(alphabet[encoded % alphabet.len()]);
                encoded /= alphabet.len();
            }
            let expected = reference(&text);
            let raw = Tokens::raw(&text).and_then(Iterator::collect::<Result<Vec<_>, _>>);
            assert_eq!(raw, expected, "raw input: {text:?}");
            if super::complete(&text).is_ok() {
                let normalized = super::normalize(&text);
                let observed = Tokens::normalized(&normalized)
                    .and_then(Iterator::collect::<Result<Vec<_>, _>>);
                assert_eq!(observed, expected, "normalized input: {text:?}");
            }
        }
    }
}

#[test]
fn escapes_unicode_whitespace_and_only_the_first_rule_are_preserved() {
    assert_eq!(
        decoded("target: a\\ b.rs c\\\\d.rs é.rs\u{2003}z.rs\nignored: other.rs\n")
            .expect("escaped paths decode"),
        ["a b.rs", "c\\d.rs", "é.rs", "z.rs"]
    );
    assert_eq!(
        decoded("target: a\\\nb.rs c\\\n d.rs\r\n").expect("continuations decode"),
        ["ab.rs", "c", "d.rs"]
    );
    assert_eq!(
        decoded("target: a\\\rb.rs").expect("a bare carriage return can be escaped"),
        ["a\rb.rs"]
    );
    assert_eq!(decoded("target: a\\\r\n"), Err(Syntax::TrailingEscape));
    for text in ["", "\nvalid: source.rs", "missing rule\nvalid: source.rs"] {
        assert_eq!(decoded(text), Err(Syntax::MissingRule));
    }
}

#[test]
fn lexical_failures_stop_after_the_same_emitted_prefix() {
    let mut tokens = Tokens::raw("target: a b\\").expect("the first rule exists");
    assert_eq!(tokens.next(), Some(Ok(Event::Character('a'))));
    assert_eq!(tokens.next(), Some(Ok(Event::Boundary)));
    assert_eq!(tokens.next(), Some(Ok(Event::Character('b'))));
    assert_eq!(tokens.next(), Some(Err(Syntax::TrailingEscape)));
    assert_eq!(tokens.next(), None);
    assert!(matches!(
        Tokens::raw("target: a\n# env-dep:BAD\n\\\n"),
        Err(Syntax::TrailingLineEscape)
    ));
}

#[test]
fn raw_environment_records_borrow_set_empty_unset_and_continued_spellings() {
    let text = "target: a.rs\n# env-dep:SET=value=more\r\n# env-dep:EMPTY=\n# env-dep:UNSET\n# env-dep:RAW=a\\\nb\n # env-dep:IGNORED=x\n";
    let values = Environment::read(text)
        .collect::<Result<Vec<_>, _>>()
        .expect("raw records decode");
    assert_eq!(
        values,
        [
            Record {
                key: "SET",
                value: Some("value=more")
            },
            Record {
                key: "EMPTY",
                value: Some("")
            },
            Record {
                key: "UNSET",
                value: None
            },
            Record {
                key: "RAW",
                value: Some("a\\")
            },
        ]
    );
    for value in values {
        assert!(value.key.as_ptr() >= text.as_ptr());
        assert!(value.key.as_ptr() < text.as_ptr().wrapping_add(text.len()));
    }
}

#[test]
fn environment_bounds_and_malformed_records_keep_precedence_and_fuse() {
    for line in [
        "# env-dep",
        "# env-dep:",
        "# env-dep:A\0B=x",
        "# env-dep:A\rB=x",
    ] {
        let mut records = Environment::read(line);
        assert_eq!(records.next(), Some(Err(Error::Malformed(line))));
        assert_eq!(records.next(), None);
    }
    // Key and value lengths are accounting targets the scanner never
    // consults; the caller's account measures them.
    assert_eq!(
        Environment::read("# env-dep:KEY=long").next(),
        Some(Ok(Record {
            key: "KEY",
            value: Some("long")
        }))
    );
}

#[test]
fn rustc_spelled_paths_round_trip_through_the_rule_reader() {
    let paths = [
        "/p/My Counter/target/debug/deps/a-1.d",
        "/p/two  spaces/#1/$HOME/a:b/é.rs",
        " leading and trailing ",
        "/p/a: b/c.rs",
    ];
    for target in paths {
        let text = format!(
            "{}: {}\n\n# env-dep:OUT_DIR={}\n",
            escape_path(target),
            paths.map(|path| escape_path(path).into_owned()).join(" "),
            escape_environment(target),
        );
        assert_eq!(decoded(&text).expect("rustc's spelling decodes"), paths, "{text:?}");
    }
    assert_eq!(escape_path("/p/a#b$c:d"), "/p/a#b$c:d");
    assert_eq!(escape_path("/p/My Counter"), "/p/My\\ Counter");
    assert_eq!(escape_environment("C:\\My Counter\n"), "C:\\\\My Counter\\n");
    // A drive's colon is part of the target.
    assert_eq!(decoded("C:/t/a.d: b.rs"), Ok(vec!["b.rs".into()]));
    assert_eq!(decoded("/p/a:b/x.d:"), Ok(vec![]));
}
