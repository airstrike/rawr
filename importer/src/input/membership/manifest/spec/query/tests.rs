use rawr_toml::document;

use super::{parse, Kind};
use crate::input::membership::manifest::{session, Error, Stop};

#[derive(Default)]
struct Meter {
    requests: Vec<document::Request>,
    refuse: Option<usize>,
}

impl document::Admission for Meter {
    type Error = usize;

    fn admit(&mut self, request: document::Request) -> Result<(), usize> {
        let index = self.requests.len();
        self.requests.push(request);
        if self.refuse == Some(index) {
            Err(index)
        } else {
            Ok(())
        }
    }
}

fn reference(query: &str) -> (Kind, String) {
    let mut output = (Kind::DefaultBranch, String::new());
    // Same fold as Cargo's GitReference::from_query; the decoder is the actual
    // pinned form_urlencoded implementation, isolated to test dependencies.
    for (key, value) in form_urlencoded::parse(query.as_bytes()) {
        let kind = match key.as_ref() {
            "branch" | "ref" => Kind::Branch,
            "rev" => Kind::Rev,
            "tag" => Kind::Tag,
            _ => continue,
        };
        output = (kind, value.into_owned());
    }
    output
}

#[test]
fn detaching_query_moves_decoded_storage_and_admits_only_borrowed_copy() {
    for (query, copied) in [("rev=abc", true), ("rev=a%20b", false)] {
        let mut meter = Meter::default();
        let mut session = session::Session::new(&mut meter);
        let result = parse(query, &mut session);
        let value = session.finish(result).unwrap();
        let pointer = value.value().as_ptr();
        let prior = meter.requests.len();
        let mut session = session::Session::new(&mut meter);
        let result = value.into_owned(&mut session);
        let owned = session.finish(result).unwrap();
        if copied {
            assert_ne!(owned.value().as_ptr(), pointer);
            assert_eq!(meter.requests.len(), prior + 3);
            assert!(matches!(
                meter.requests[prior],
                document::Request::String(3)
            ));
            assert!(matches!(
                meter.requests[prior + 1],
                document::Request::Items(1)
            ));
            assert!(matches!(
                meter.requests[prior + 2],
                document::Request::Allocation {
                    bytes: 3,
                    alignment: 1
                }
            ));
        } else {
            assert_eq!(owned.value().as_ptr(), pointer);
            assert_eq!(meter.requests.len(), prior);
        }
    }
}

fn compare(query: &str) {
    let mut meter = Meter::default();
    let mut session = session::Session::new(&mut meter);
    let result = parse(query, &mut session);
    let actual = session.finish(result).unwrap();
    let expected = reference(query);
    assert_eq!(
        (actual.kind(), actual.value()),
        (expected.0, expected.1.as_str()),
        "{query:?}"
    );
}

#[test]
fn direct_pinned_decoder_preserves_order_escapes_and_lossy_utf8() {
    for query in [
        "",
        "&&&",
        "branch",
        "branch=",
        "tag=x&rev=y&branch=z",
        "branch=x&ignore=last",
        "ref=legacy",
        "%62ranch=main",
        "br+anch=x",
        "branch=a+b%2Bc",
        "rev=a=b=c",
        "rev=%",
        "rev=%2",
        "rev=%xx",
        "tag=%F0%9F%92%A9",
        "tag=%F0%9F%92",
        "tag=%ED%A0%80",
        "tag=%FF%FF",
        "tag=%00",
        "tag=%E2%28%A1",
        "branch=日本語",
        "branch=a%0Ab",
        "branch=before&%72ev=after",
        "%FFbranch=x",
        "tag=one&&&tag=two",
        "unknown=%FF&branch=ok",
        "branch=%FF&tag=final",
    ] {
        compare(query);
    }
    for byte in 0..=255 {
        compare(&format!("branch=pre%{byte:02X}post"));
        compare(&format!("%{byte:02x}rev=ignored&rev=last"));
    }
    for first in 0..=255 {
        for second in [0, 0x28, 0x7f, 0x80, 0xbf, 0xc0, 0xf0, 0xff] {
            compare(&format!("ref=%{first:02X}%{second:02X}"));
        }
    }
}

#[test]
fn unchanged_and_overwritten_values_need_no_owned_string() {
    let mut meter = Meter::default();
    let mut session = session::Session::new(&mut meter);
    let result = parse("tag=%FF&unknown=%FF&branch=日本語%xx", &mut session);
    let value = session.finish(result).unwrap();
    assert_eq!(value.kind(), Kind::Branch);
    assert!(matches!(
        value.value,
        std::borrow::Cow::Borrowed("日本語%xx")
    ));
    assert!(meter.requests.is_empty());
}

#[test]
fn decoded_and_repaired_layouts_are_exact_and_cumulative() {
    let mut meter = Meter::default();
    let mut session = session::Session::new(&mut meter);
    let result = parse("tag=%F0%9F%92", &mut session);
    let value = session.finish(result).unwrap();
    assert_eq!(value.value(), "\u{fffd}");
    assert_eq!(meter.requests.len(), 6);
    for (requests, expected) in meter.requests.as_chunks::<3>().0.iter().zip([3, 3]) {
        assert!(matches!(requests[0], document::Request::String(n) if n == expected));
        assert!(matches!(requests[1], document::Request::Items(1)));
        assert!(
            matches!(requests[2], document::Request::Allocation { bytes, alignment: 1 } if bytes == expected)
        );
    }
}

#[test]
fn every_refusal_latches_and_prevents_even_borrowed_followup() {
    for refusal in 0..6 {
        let mut meter = Meter {
            refuse: Some(refusal),
            ..Meter::default()
        };
        let mut session = session::Session::new(&mut meter);
        assert!(matches!(parse("tag=%FF", &mut session), Err(Stop::Refused)));
        assert!(matches!(
            parse("branch=borrowed", &mut session),
            Err(Stop::Refused)
        ));
        let result = session.finish(Ok(()));
        assert!(matches!(result, Err(Error::Admission(index)) if index == refusal));
        assert_eq!(meter.requests.len(), refusal + 1);
    }
}

#[test]
fn original_account_retains_prior_usage_and_exact_expansion_floor() {
    use crate::cargo;
    use crate::input;
    use crate::input::membership;
    use crate::input::membership::manifest::tests::one_short;
    use crate::node;

    for extra in [3, 4] {
        let limits = cargo::Limits::default();
        let mut allocation = input::Allocation::new(&limits);
        allocation.bytes(7).unwrap();
        allocation.byte_limit = 7 + extra;
        let graph = node::GraphBudget::new();
        graph.paths(node::MAX_PATH_CHECKS_PER_GRAPH).unwrap();
        graph
            .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
            .unwrap();
        graph
            .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
            .unwrap();
        let mut budget = membership::Budget {
            graph: &graph,
            allocation: &mut allocation,
        };
        let mut session = session::Session::new(&mut budget);
        let result = parse("rev=%FF", &mut session);
        let result = session.finish(result);
        // The four-byte expansion past a three-byte remainder is admitted
        // and recorded at the exact total; nothing latches.
        assert_eq!(result.unwrap().value(), "\u{fffd}");
        assert_eq!(allocation.usage().bytes, 7 + 4);
        assert!(allocation.check().is_ok(), "accounting targets never latch");
        let exceeded = allocation.exceedances();
        if extra == 3 {
            assert!(one_short(&exceeded, input::INPUT_BYTES_RESOURCE, 7 + 4));
        } else {
            assert!(!exceeded.is_exceeded());
        }
    }
}
