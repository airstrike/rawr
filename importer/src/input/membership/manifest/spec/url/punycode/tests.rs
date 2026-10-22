use std::alloc::Layout;

use rawr_toml::document;

use super::{decoder, encode, Decoder, Invalid};
use crate::input::membership::manifest::{session, Error, Stop};

mod reference;

#[derive(Default)]
struct Account {
    requests: Vec<document::Request>,
    refusal: Option<usize>,
}

impl document::Admission for Account {
    type Error = usize;

    fn admit(&mut self, request: document::Request) -> Result<(), usize> {
        let index = self.requests.len();
        self.requests.push(request);
        if self.refusal == Some(index) {
            Err(index)
        } else {
            Ok(())
        }
    }
}

fn decoded(input: &[u8]) -> Result<String, Invalid> {
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let mut decoder = Decoder::new();
    let result = decoder.bytes(input, &mut session);
    session.finish(result).unwrap().map(Iterator::collect)
}

#[test]
fn internal_bytes_and_chars_keep_distinct_case_and_delimiter_semantics() {
    for (input, expected) in [
        ("", ""),
        ("Bach-", "bach"),
        ("BCHER-KVA", "bücher"),
        ("tda", "ü"),
        ("A--", "a-"),
        ("a-a", "\u{80}a"),
        ("aaaaa", "\u{80}\u{80}\u{80}\u{80}\u{80}"),
    ] {
        assert_eq!(decoded(input.as_bytes()).unwrap(), expected, "{input}");
    }
    for input in ["-", "-a", "bcher-KVA", "a-!", "a_", "9"] {
        let chars = input.chars().collect::<Vec<_>>();
        let mut account = Account::default();
        let mut session = session::Session::new(&mut account);
        let mut decoder = Decoder::new();
        let result = decoder.chars(&chars, &mut session);
        assert_eq!(session.finish(result).unwrap().unwrap_err(), Invalid::Digit);
    }
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let mut decoder = Decoder::new();
    for (input, expected) in [("Bach-", "Bach"), ("Ü-", "Ü"), ("BCHER-kva", "BüCHER")] {
        let chars = input.chars().collect::<Vec<_>>();
        let value = decoder.chars(&chars, &mut session).unwrap().unwrap();
        assert_eq!(value.collect::<String>(), expected);
    }
    assert_eq!(decoded(&[0xff, b'-']).unwrap(), "ÿ");
    session.finish(Ok(())).unwrap();
}

#[test]
fn encoder_keeps_ascii_spelling_delimiter_and_non_bmp_digits() {
    for (input, expected) in [
        ("", ""),
        ("Bach", "Bach-"),
        ("b\u{fc}cher", "bcher-kva"),
        ("ü", "tda"),
        ("üëäö♥", "4can8av2009b"),
        ("\0-", "\0--"),
    ] {
        let chars = input.chars().collect::<Vec<_>>();
        let mut account = Account::default();
        let mut session = session::Session::new(&mut account);
        let result = encode(&chars, &mut session);
        assert_eq!(
            session.finish(result).unwrap().unwrap().as_bytes(),
            expected.as_bytes()
        );
    }
    for input in [vec![char::MAX], vec!['a', char::MAX, '\u{80}', char::MAX]] {
        let mut account = Account::default();
        let mut session = session::Session::new(&mut account);
        let encoded = encode(&input, &mut session).unwrap().unwrap();
        let mut decoder = Decoder::new();
        let decoded = decoder
            .bytes(encoded.as_bytes(), &mut session)
            .unwrap()
            .unwrap();
        assert_eq!(decoded.collect::<Vec<_>>(), input);
        session.finish(Ok(())).unwrap();
    }
}

#[test]
fn reusable_scratch_retains_capacity_and_returns_exact_iterator_lengths() {
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let mut decoder = Decoder::new();
    for input in [b"aaaaa".as_slice(), b"bcher-kva", b"Bach-", b"a"] {
        let mut decoded = decoder.bytes(input, &mut session).unwrap().unwrap();
        let length = decoded.len();
        assert_eq!(decoded.size_hint(), (length, Some(length)));
        for remaining in (0..length).rev() {
            assert!(decoded.next().is_some());
            assert_eq!(decoded.len(), remaining);
        }
        assert_eq!(decoded.next(), None);
        assert_eq!(decoded.next(), None);
    }
    session.finish(Ok(())).unwrap();
    let layouts = account
        .requests
        .iter()
        .filter_map(|request| match request {
            document::Request::Allocation { bytes, alignment } => Some((*bytes, *alignment)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let expected = [4, 8].map(|capacity| {
        let layout = Layout::array::<decoder::Insertion>(capacity).unwrap();
        (layout.size(), layout.align())
    });
    assert_eq!(layouts, expected);
    assert_eq!(
        account
            .requests
            .iter()
            .filter(|request| matches!(request, document::Request::Items(1)))
            .count(),
        7
    );
    assert!(!account
        .requests
        .iter()
        .any(|request| matches!(request, document::Request::String(_))));
}

#[test]
fn semantic_failure_retains_partial_scratch_without_exposing_partial_decode() {
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let mut decoder = Decoder::new();
    assert_eq!(
        decoder.bytes(b"aaaaa!", &mut session).unwrap().unwrap_err(),
        Invalid::Digit
    );
    assert_eq!(
        decoder
            .bytes(b"99999999999999999999", &mut session)
            .unwrap()
            .unwrap_err(),
        Invalid::Overflow
    );
    assert_eq!(
        decoder
            .bytes(b"tda", &mut session)
            .unwrap()
            .unwrap()
            .collect::<String>(),
        "ü"
    );
    session.finish(Ok(())).unwrap();
    assert_eq!(
        account
            .requests
            .iter()
            .filter(|request| matches!(request, document::Request::Allocation { .. }))
            .count(),
        2
    );
    assert_eq!(
        account
            .requests
            .iter()
            .filter(|request| matches!(request, document::Request::Items(1)))
            .count(),
        8
    );
}

#[test]
fn work_guards_match_uts46_without_a_dns_or_ascii_length_limit() {
    assert_eq!(decoded(&vec![b'a'; 2000]).unwrap().chars().count(), 2000);
    assert_eq!(decoded(&vec![b'a'; 2001]), Err(Invalid::Length));
    let oversized = vec!['a'; 2001];
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let mut decoder = Decoder::new();
    let result = decoder.chars(&oversized, &mut session);
    assert_eq!(
        session.finish(result).unwrap().unwrap_err(),
        Invalid::Length
    );
    assert_eq!(account.requests, [document::Request::Items(1)]);
    for (input, expected) in [
        (vec!['ü'; 1000], None),
        (vec!['ü'; 1001], Some(Invalid::Length)),
        (vec!['a'; 1001], None),
        (vec!['a'; 4000], Some(Invalid::Overflow)),
    ] {
        let mut account = Account::default();
        let mut session = session::Session::new(&mut account);
        let result = encode(&input, &mut session);
        assert_eq!(session.finish(result).unwrap().err(), expected);
        if expected == Some(Invalid::Length) {
            assert_eq!(account.requests, [document::Request::Items(1)]);
        }
    }
}

#[test]
fn every_encoder_and_decoder_admission_refusal_is_terminal() {
    for encode_input in [false, true] {
        let mut full = Account::default();
        let mut session = session::Session::new(&mut full);
        let mut decoder = Decoder::new();
        if encode_input {
            assert!(encode(&['B', 'ü', 'c', 'h', 'e', 'r'], &mut session)
                .unwrap()
                .is_ok());
        } else {
            assert!(decoder.bytes(b"aaaaa!", &mut session).unwrap().is_err());
        }
        session.finish(Ok(())).unwrap();
        for refusal in 0..full.requests.len() {
            let mut account = Account {
                refusal: Some(refusal),
                ..Account::default()
            };
            let mut session = session::Session::new(&mut account);
            let mut decoder = Decoder::new();
            let result = if encode_input {
                encode(&['B', 'ü', 'c', 'h', 'e', 'r'], &mut session).map(|_| ())
            } else {
                decoder.bytes(b"aaaaa!", &mut session).map(|_| ())
            };
            assert!(matches!(result, Err(Stop::Refused)));
            assert!(matches!(encode(&[], &mut session), Err(Stop::Refused)));
            assert!(matches!(
                decoder.bytes(b"", &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(
                decoder.chars(&[], &mut session),
                Err(Stop::Refused)
            ));
            assert!(
                matches!(session.finish(Ok(())), Err(Error::Admission(index)) if index == refusal)
            );
            assert_eq!(account.requests, full.requests[..=refusal]);
        }
    }
}

#[test]
fn original_account_keeps_prior_usage_exact_layout_and_string_floors() {
    use crate::input;
    use crate::input::membership::manifest::tests::one_short;
    use crate::input::membership::Budget;
    use crate::node;

    let graph = node::GraphBudget::new();
    graph.paths(node::MAX_PATH_CHECKS_PER_GRAPH).unwrap();
    graph
        .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
        .unwrap();
    graph
        .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
        .unwrap();
    // `Bücher` encodes to nine ASCII bytes: growth requests 4 + 8 + 16.
    // One short of an item, byte or string target still admits the
    // whole encoding and records only that axis, with the exact total as its
    // high water and the one-short figure as its target.
    for (items, bytes, string, exceeded) in [
        (16, 47, 9, None),
        (15, 47, 9, Some((input::INPUT_ITEMS_RESOURCE, 16))),
        (16, 46, 9, Some((input::INPUT_BYTES_RESOURCE, 47))),
        (16, 47, 8, Some(("membership decoded TOML string bytes", 9))),
    ] {
        let mut allocation = input::Allocation::with_limits(items, bytes, string);
        allocation.items(7).unwrap();
        allocation.bytes(19).unwrap();
        let mut budget = Budget {
            graph: &graph,
            allocation: &mut allocation,
        };
        let mut session = session::Session::new(&mut budget);
        let result = encode(&['B', 'ü', 'c', 'h', 'e', 'r'], &mut session);
        let result = session.finish(result);
        assert_eq!(result.unwrap().unwrap().as_bytes(), b"Bcher-kva");
        assert_eq!(allocation.usage.items, 16);
        assert_eq!(allocation.usage.bytes, 47);
        assert!(allocation.check().is_ok(), "accounting targets never latch");
        let record = allocation.exceedances();
        match exceeded {
            Some((resource, total)) => {
                assert!(one_short(&record, resource, total), "{record:?}");
                let axis = record.axis(resource).unwrap();
                assert_eq!(axis.target, total as u64 - 1);
                assert_eq!(axis.first_requested, total as u64);
            }
            None => assert!(!record.is_exceeded()),
        }
    }

    let scratch_bytes = 19 + Layout::array::<decoder::Insertion>(4).unwrap().size();
    for (items, bytes, exceeded) in [
        (9, scratch_bytes, None),
        (8, scratch_bytes, Some((input::INPUT_ITEMS_RESOURCE, 9))),
        (
            9,
            scratch_bytes - 1,
            Some((input::INPUT_BYTES_RESOURCE, scratch_bytes)),
        ),
    ] {
        let mut allocation = input::Allocation::with_limits(items, bytes, 0);
        allocation.items(7).unwrap();
        allocation.bytes(19).unwrap();
        let mut budget = Budget {
            graph: &graph,
            allocation: &mut allocation,
        };
        let mut session = session::Session::new(&mut budget);
        let mut decoder = Decoder::new();
        let result = (|| {
            assert_eq!(
                decoder
                    .bytes(b"tda", &mut session)?
                    .unwrap()
                    .collect::<String>(),
                "ü"
            );
            assert_eq!(
                decoder.bytes(b"a", &mut session)?.unwrap().next(),
                Some('\u{80}')
            );
            Ok(())
        })();
        session.finish(result).unwrap();
        assert_eq!(allocation.usage.items, 9);
        assert_eq!(allocation.usage.bytes, scratch_bytes);
        assert!(allocation.check().is_ok(), "accounting targets never latch");
        match exceeded {
            Some((resource, total)) => {
                assert!(one_short(&allocation.exceedances(), resource, total))
            }
            None => assert!(!allocation.exceedances().is_exceeded()),
        }
    }
}
