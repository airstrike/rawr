use super::*;
use std::ffi::OsStr;

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

#[test]
fn sanitizer_matches_direct_pinned_observations() {
    for line in include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/reference/manifest/name-observations.jsonl"
    ))
    .lines()
    {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let raw = row["source"].as_str().unwrap();
        let expected = row["output"].as_str().unwrap();
        let mut account = Account::default();
        let mut session = Session::new(&mut account);
        let result = sanitize(raw.as_bytes(), &mut session);
        assert_eq!(session.finish(result).unwrap(), expected, "{raw:?}");
    }
}

#[test]
fn native_stems_preserve_namespace_and_placeholder_rules() {
    for (file, expected) in [
        (&b"/scripts/hello.rs"[..], "hello"),
        (&b"/scripts/123 hello.world.rs"[..], "hello-world"),
        (&b"/scripts/a b_c!.rs"[..], "a_b_c_"),
        (&b"/scripts/123.rs"[..], "package"),
        (
            &b"/scripts/::a::::b::.rs"[..],
            "package::a::package::b::package",
        ),
        (&b"/scripts/a:::b.rs"[..], "a::b"),
        (&b"/scripts/a:b.rs"[..], "a-b"),
        (&b"/scripts/.hidden"[..], "hidden"),
        (&b"/scripts/foo.tar.rs"[..], "foo-tar"),
        (&b"/scripts/a\xff\xfe.rs"[..], "a--"),
        (&b"/scripts/a\xe2\x82.rs"[..], "a-"),
        (&b"/scripts/\xff\xfea.rs"[..], "a"),
        (&b"/scripts/a\xff_b.rs"[..], "a__b"),
        ("/scripts/🦀é漢字.rs".as_bytes(), "é漢字"),
        ("/scripts/a\u{301}!b.rs".as_bytes(), "a\u{301}-b"),
        ("/scripts/\u{301}a.rs".as_bytes(), "a"),
    ] {
        let mut account = Account::default();
        let mut session = Session::new(&mut account);
        let result = embedded(Path::new(OsStr::from_bytes(file)), &mut session);
        assert_eq!(session.finish(result).unwrap(), expected, "{file:?}");
    }
}

#[test]
fn streamed_lossy_decoding_matches_std_invalid_sequence_grouping() {
    for bytes in [
        &b"a\xff\xfe_b"[..],
        &b"a\xe2\x82"[..],
        &b"\xf0\x90\x80a"[..],
        &b"\xed\xa0\x80"[..],
        &b"\xc0\xaf"[..],
        &b"\xf4\x90\x80\x80"[..],
        &b"\xe2::\x82"[..],
        "é漢字🦀".as_bytes(),
    ] {
        assert_eq!(
            lossy(bytes).collect::<String>(),
            String::from_utf8_lossy(bytes)
        );
    }
}

#[test]
fn every_admission_refusal_is_terminal_before_output_allocation() {
    let path = Path::new("/scripts/a b_c.rs");
    let mut baseline = Account::default();
    let mut session = Session::new(&mut baseline);
    let result = embedded(path, &mut session);
    assert_eq!(session.finish(result).unwrap(), "a_b_c");
    for refusal in 0..baseline.requests.len() {
        let mut account = Account {
            refusal: Some(refusal),
            ..Account::default()
        };
        let mut session = Session::new(&mut account);
        storage::ALLOCATIONS.with(|count| count.set(0));
        assert!(embedded(path, &mut session).is_err());
        assert!(embedded(path, &mut session).is_err());
        assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), 0);
        assert!(matches!(session.finish(Ok(())), Err(Error::Admission(index)) if index == refusal));
        assert_eq!(account.requests, baseline.requests[..=refusal]);
    }
}

#[test]
fn original_account_retains_prior_usage_and_exact_output_layout() {
    use crate::input::membership::manifest::tests::admits_one_short;
    use crate::input::membership::Budget;
    use crate::{input, node};

    let run = |items, bytes, string| {
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::with_limits(items, bytes, string);
        allocation.items(7).unwrap();
        allocation.bytes(31).unwrap();
        let mut budget = Budget {
            graph: &graph,
            allocation: &mut allocation,
        };
        let mut session = Session::new(&mut budget);
        let result = embedded(Path::new("/scripts/123.rs"), &mut session);
        let ok = session.finish(result).is_ok();
        assert!(allocation.check().is_ok(), "accounting targets never latch");
        (ok, allocation.usage(), allocation.exceedances())
    };
    let (ok, usage, exceeded) = run(8, 38, 7);
    assert!(ok);
    assert_eq!(usage.items, 8);
    assert_eq!(usage.bytes, 38);
    assert!(!exceeded.is_exceeded());
    // One short of an aggregate target admits and records that axis.
    admits_one_short(run(7, 38, 7), usage, input::INPUT_ITEMS_RESOURCE, 8);
    admits_one_short(run(8, 37, 7), usage, input::INPUT_BYTES_RESOURCE, 38);
    // One short of the 7-byte output string records the decoded-string
    // axis; the same output is still produced and owned.
    let (ok, spent, exceeded) = run(8, 38, 6);
    admits_one_short(
        (ok, spent, exceeded),
        usage,
        "membership decoded TOML string bytes",
        7,
    );
    let axis = exceeded
        .axis("membership decoded TOML string bytes")
        .unwrap();
    assert_eq!((axis.target, axis.first_requested), (6, 7));
}

#[test]
fn missing_file_stem_is_an_internal_precondition_failure() {
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    let result = embedded(Path::new("/"), &mut session);
    assert!(matches!(session.finish(result), Err(Error::State)));
    assert!(account.requests.is_empty());
}
