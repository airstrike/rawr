use std::sync::Arc;

use super::super::{contiguous, dfa, noncontiguous, tests::Count};
use super::{Engine, Match};
use crate::input::membership::manifest::{reference::Account, session::Session, Stop};

fn collect(engine: &Engine, text: &[u8]) -> Vec<Match> {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut cursor = engine.overlapping(text, &mut session).unwrap();
    let mut matches = Vec::new();
    while let Some(matched) = cursor.next(&mut session).unwrap() {
        matches.push(matched);
    }
    matches
}

fn engines(patterns: &[&str]) -> [Engine; 3] {
    let texts: Vec<_> = patterns.iter().map(|text| (*text).to_owned()).collect();
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let nfa = noncontiguous::Nfa::build(&texts, &mut session)
        .ok()
        .unwrap();
    let dfa = dfa::Dfa::build(&nfa, &mut session).ok().unwrap();
    let compact = contiguous::Nfa::build(&nfa, &mut session).ok().unwrap();
    [
        Engine::Dfa(Arc::new(dfa)),
        Engine::Contiguous(Arc::new(compact)),
        Engine::Noncontiguous(Arc::new(nfa)),
    ]
}

#[test]
fn all_engines_preserve_empty_duplicate_inherited_and_prefiltered_overlap_order() {
    // Confirmed against pinned aho-corasick 1.1.4: failure inheritance plus
    // the root copy preserves repeated empty IDs; do not normalize them here.
    let expected: Vec<_> = [
        (5, 0, 0),
        (5, 1, 1),
        (5, 2, 2),
        (5, 3, 3),
        (5, 3, 3),
        (1, 1, 4),
        (0, 2, 4),
        (4, 2, 4),
        (5, 4, 4),
        (5, 4, 4),
        (5, 4, 4),
        (5, 5, 5),
        (5, 5, 5),
        (2, 2, 6),
        (5, 6, 6),
        (5, 6, 6),
    ]
    .into_iter()
    .map(|(pattern, start, end)| Match {
        pattern,
        start,
        end,
    })
    .collect();
    for engine in engines(&["he", "she", "hers", "his", "he", ""]) {
        assert_eq!(collect(&engine, b"ushers"), expected);
        assert_eq!(
            collect(&engine, b""),
            [Match {
                pattern: 5,
                start: 0,
                end: 0
            }]
        );
    }
    // The single-pattern memmem path and rare-byte offset path both skip
    // irrelevant spans before returning to actual automaton confirmation.
    for engine in engines(&["needle"]) {
        assert_eq!(
            collect(&engine, b"--------needle-needle"),
            [
                Match {
                    pattern: 0,
                    start: 8,
                    end: 14
                },
                Match {
                    pattern: 0,
                    start: 15,
                    end: 21
                },
            ]
        );
        assert!(collect(&engine, b"--------------------").is_empty());
    }
    for engine in engines(&["t\u{1}", "tt\u{1}"]) {
        assert_eq!(
            collect(&engine, b"--tt\x01--t\x01"),
            [
                Match {
                    pattern: 1,
                    start: 2,
                    end: 5
                },
                Match {
                    pattern: 0,
                    start: 3,
                    end: 5
                },
                Match {
                    pattern: 0,
                    start: 7,
                    end: 9
                },
            ]
        );
    }
}

#[test]
fn default_selection_and_query_refusal_preserve_the_original_account() {
    let short = vec!["needle".to_owned()];
    let engine = Engine::build(&short, &mut Session::new(&mut Account))
        .unwrap()
        .unwrap();
    assert!(matches!(&engine, Engine::Dfa(_)));
    let many: Vec<_> = (0..101).map(|i| format!("prefix{i:03}")).collect();
    let compact = Engine::build(&many, &mut Session::new(&mut Account))
        .unwrap()
        .unwrap();
    assert!(matches!(&compact, Engine::Contiguous(_)));
    assert_eq!(
        collect(&compact, b"--prefix100"),
        [Match {
            pattern: 100,
            start: 2,
            end: 11
        }]
    );

    let mut account = Count::default();
    let mut session = Session::new(&mut account);
    let mut cursor = engine
        .overlapping(b"--------needle-needle", &mut session)
        .unwrap();
    while cursor.next(&mut session).unwrap().is_some() {}
    let baseline = account.requests;
    for index in [0, baseline.len() / 2, baseline.len() - 1] {
        let mut account = Count {
            refuse: Some(index),
            ..Count::default()
        };
        let mut session = Session::new(&mut account);
        let result = (|| {
            let mut cursor = engine.overlapping(b"--------needle-needle", &mut session)?;
            while cursor.next(&mut session)?.is_some() {}
            Ok(())
        })();
        assert!(matches!(result, Err(Stop::Refused)));
        assert!(matches!(
            engine.overlapping(b"", &mut session),
            Err(Stop::Refused)
        ));
        assert_eq!(account.requests, baseline[..index]);
        assert_eq!(account.calls, index + 1);
    }
}
